use opencad::{
    Drawing, DxfDocument, Entity, Line3, Mesh3, MeshBooleanOp, Point2, Point3, mesh_boolean,
    read_obj, read_stl, render_svg, write_dxf, write_obj, write_stl_ascii, write_stl_binary,
};
use std::env;
use std::error::Error;
use std::fs;
use std::process::ExitCode;

const USAGE: &str = "Usage: opencad <output.svg> <x1> <y1> <x2> <y2> [<x> <y> | --undo | --close]... [--break <x1> <y1> ...]\n       opencad line3 <output.dxf> <sx> <sy> <sz> <ex> <ey> <ez> [<sx> <sy> <sz> <ex> <ey> <ez>]...\n       opencad dxf-copy <input.dxf> <output.dxf>\n       opencad dxf-set-line <input.dxf> <output.dxf> <line-number> <sx> <sy> <sz> <ex> <ey> <ez>\n       opencad import-obj <input.obj> <output.dxf>\n       opencad import-stl <input.stl> <output.dxf>\n       opencad export-obj <input.dxf> <output.obj>\n       opencad export-stl <input.dxf> <output.stl> [--ascii]\n       opencad mesh-boolean <union|subtract|intersect> <a.dxf> <b.dxf> <output.dxf>\n--undo removes the latest segment in the current chain; --close connects its last point to its first (after two segments); --break starts a separate chain. Positive Y points upward.\nline3 writes independent 3D LINE entities. dxf-copy preserves the supported R12 ASCII DXF document, including LINE, world-XY CIRCLE / ARC, and straight XY POLYLINE entities at a common elevation on named layers and imported metadata. dxf-set-line replaces one LINE by its one-based line number. See README.md for the supported format and desktop editor.";

fn parse_lines3(tokens: &[String]) -> Result<Vec<Line3>, Box<dyn Error>> {
    if tokens.is_empty() || !tokens.len().is_multiple_of(6) {
        return Err("line3 requires six coordinates per line: sx sy sz ex ey ez".into());
    }
    tokens
        .chunks_exact(6)
        .enumerate()
        .map(|(index, coordinates)| {
            let mut values = [0.0; 6];
            for (value, token) in values.iter_mut().zip(coordinates) {
                *value = token.parse().map_err(|_| {
                    format!("line {} has an invalid coordinate: {token}", index + 1)
                })?;
            }
            let start = Point3::new(values[0], values[1], values[2])
                .map_err(|error| format!("line {} start: {error}", index + 1))?;
            let end = Point3::new(values[3], values[4], values[5])
                .map_err(|error| format!("line {} end: {error}", index + 1))?;
            Ok(Line3::new(start, end))
        })
        .collect()
}

fn parse_drawing(tokens: &[String]) -> Result<Drawing, Box<dyn Error>> {
    let mut drawing = Drawing::new();
    for (chain_index, chain) in tokens.split(|token| token == "--break").enumerate() {
        if chain.is_empty() {
            return Err(format!(
                "chain {} needs at least two X Y pairs; see --help",
                chain_index + 1
            )
            .into());
        }

        let first_segment_index = drawing.segments().len();
        let mut token_index = 0;
        let mut point_index = 0;
        let mut closed = false;
        while token_index < chain.len() {
            if chain[token_index] == "--undo" {
                drawing
                    .undo_last_segment()
                    .ok_or_else(|| format!("chain {} has no segment to undo", chain_index + 1))?;
                token_index += 1;
                continue;
            }
            if chain[token_index] == "--close" {
                if token_index + 1 != chain.len() {
                    return Err(format!(
                        "chain {}: --close must end the chain; use --break before another chain",
                        chain_index + 1
                    )
                    .into());
                }
                drawing
                    .close_chain()
                    .map_err(|error| format!("chain {}: {error}", chain_index + 1))?;
                closed = true;
                break;
            }
            if token_index + 1 == chain.len()
                || chain[token_index + 1] == "--undo"
                || chain[token_index + 1] == "--close"
            {
                return Err(format!(
                    "chain {} needs complete X Y pairs; see --help",
                    chain_index + 1
                )
                .into());
            }

            let x: f64 = chain[token_index].parse().map_err(|_| {
                format!(
                    "chain {}, point {} has an invalid X coordinate: {}",
                    chain_index + 1,
                    point_index + 1,
                    chain[token_index]
                )
            })?;
            let y: f64 = chain[token_index + 1].parse().map_err(|_| {
                format!(
                    "chain {}, point {} has an invalid Y coordinate: {}",
                    chain_index + 1,
                    point_index + 1,
                    chain[token_index + 1]
                )
            })?;
            let point = Point2::new(x, y).map_err(|error| {
                format!(
                    "chain {}, point {}: {error}",
                    chain_index + 1,
                    point_index + 1
                )
            })?;
            drawing.click(point);
            point_index += 1;
            token_index += 2;
        }
        if !closed {
            if drawing.segments().len() == first_segment_index {
                return Err(format!(
                    "chain {} needs at least one remaining segment; see --help",
                    chain_index + 1
                )
                .into());
            }
            drawing.finish_chain();
        }
    }

    Ok(drawing)
}

fn document_meshes(document: &DxfDocument) -> Result<Vec<Mesh3>, Box<dyn Error>> {
    let mut meshes = Vec::new();
    for entity in document.entities() {
        collect_entity_meshes(entity, document, &mut meshes, 0)?;
    }
    if meshes.is_empty() {
        return Err("DXF contains no convertible Face3, Mesh, or block geometry".into());
    }
    Ok(meshes)
}

fn merge_meshes(meshes: Vec<Mesh3>) -> Result<Mesh3, Box<dyn Error>> {
    let faces = meshes.into_iter().flat_map(Mesh3::into_faces).collect();
    Ok(Mesh3::new(faces)?)
}

fn collect_entity_meshes(
    entity: &Entity,
    document: &DxfDocument,
    meshes: &mut Vec<Mesh3>,
    depth: usize,
) -> Result<(), Box<dyn Error>> {
    if depth > 32 {
        return Err("block expansion exceeds the recursion limit".into());
    }
    match entity {
        Entity::Face3(face) => meshes.push(Mesh3::new(vec![*face])?),
        Entity::Mesh(mesh) => meshes.push(mesh.clone()),
        Entity::Insert(insert) => {
            let block = document
                .block(insert.block_name())
                .ok_or_else(|| format!("unresolved block INSERT: {}", insert.block_name()))?;
            for expanded in insert.explode(block)? {
                collect_entity_meshes(&expanded, document, meshes, depth + 1)?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn run() -> Result<(), Box<dyn Error>> {
    let args: Vec<String> = env::args().skip(1).collect();
    if args.len() == 1 && (args[0] == "--help" || args[0] == "-h") {
        println!("{USAGE}");
        return Ok(());
    }
    if args.first().is_some_and(|arg| arg == "line3") {
        if args.len() < 8 {
            return Err(USAGE.into());
        }
        let lines = parse_lines3(&args[2..])?;
        fs::write(&args[1], write_dxf(&lines))?;
        return Ok(());
    }
    if args.first().is_some_and(|arg| arg == "dxf-copy") {
        if args.len() != 3 {
            return Err(USAGE.into());
        }
        let input = fs::read_to_string(&args[1])?;
        let document = DxfDocument::parse(&input)?;
        fs::write(&args[2], document.to_dxf())?;
        return Ok(());
    }
    if args.first().is_some_and(|arg| arg == "dxf-set-line") {
        if args.len() != 10 {
            return Err(USAGE.into());
        }
        let index = args[3]
            .parse::<usize>()?
            .checked_sub(1)
            .ok_or("line numbers start at 1")?;
        let line = parse_lines3(&args[4..])?[0];
        let input = fs::read_to_string(&args[1])?;
        let mut document = DxfDocument::parse(&input)?;
        document
            .replace_line(index, line)
            .ok_or("line number is outside the document")?;
        fs::write(&args[2], document.to_dxf())?;
        return Ok(());
    }
    if args.first().is_some_and(|arg| arg == "import-obj") {
        if args.len() != 3 {
            return Err(USAGE.into());
        }
        let meshes = read_obj(&fs::read_to_string(&args[1])?)?;
        let mut document = DxfDocument::default();
        for mesh in meshes {
            document.push(Entity::Mesh(mesh));
        }
        fs::write(&args[2], document.to_dxf())?;
        return Ok(());
    }
    if args.first().is_some_and(|arg| arg == "import-stl") {
        if args.len() != 3 {
            return Err(USAGE.into());
        }
        let mesh = read_stl(&fs::read(&args[1])?)?;
        let mut document = DxfDocument::default();
        document.push(Entity::Mesh(mesh));
        fs::write(&args[2], document.to_dxf())?;
        return Ok(());
    }
    if args
        .first()
        .is_some_and(|arg| arg == "export-obj" || arg == "export-stl")
    {
        if args.len() < 3 || args.len() > 4 {
            return Err(USAGE.into());
        }
        let document = DxfDocument::parse(&fs::read_to_string(&args[1])?)?;
        let meshes = document_meshes(&document)?;
        if args[0] == "export-obj" {
            fs::write(&args[2], write_obj(&meshes)?)?;
        } else if args.get(3).is_some_and(|option| option == "--ascii") {
            fs::write(&args[2], write_stl_ascii(&meshes)?)?;
        } else {
            fs::write(&args[2], write_stl_binary(&meshes)?)?;
        }
        return Ok(());
    }
    if args.first().is_some_and(|arg| arg == "mesh-boolean") {
        if args.len() != 5 {
            return Err(USAGE.into());
        }
        let operation = match args[1].to_ascii_lowercase().as_str() {
            "union" => MeshBooleanOp::Union,
            "subtract" => MeshBooleanOp::Subtract,
            "intersect" => MeshBooleanOp::Intersect,
            _ => return Err("mesh-boolean operation must be union, subtract, or intersect".into()),
        };
        let left_doc = DxfDocument::parse(&fs::read_to_string(&args[2])?)?;
        let right_doc = DxfDocument::parse(&fs::read_to_string(&args[3])?)?;
        let left = merge_meshes(document_meshes(&left_doc)?)?;
        let right = merge_meshes(document_meshes(&right_doc)?)?;
        let result = mesh_boolean(&left, &right, operation)?;
        let mut document = DxfDocument::default();
        document.push(Entity::Mesh(result));
        fs::write(&args[4], document.to_dxf())?;
        return Ok(());
    }
    if args.len() < 5 {
        return Err(USAGE.into());
    }

    let drawing = parse_drawing(&args[1..])?;
    let svg = render_svg(&drawing)?;
    fs::write(&args[0], svg)?;
    Ok(())
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("opencad: {error}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn break_creates_separate_line_chains() {
        let tokens = ["0", "0", "10", "0", "--break", "20", "20", "30", "20"].map(String::from);
        let drawing = parse_drawing(&tokens).unwrap();

        assert_eq!(drawing.segments().len(), 2);
        assert_eq!(drawing.segments()[0].end(), Point2::new(10.0, 0.0).unwrap());
        assert_eq!(
            drawing.segments()[1].start(),
            Point2::new(20.0, 20.0).unwrap()
        );
    }

    #[test]
    fn break_requires_another_complete_chain() {
        let tokens = ["0", "0", "10", "0", "--break"].map(String::from);
        let error = parse_drawing(&tokens).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("chain 2 needs at least two X Y pairs")
        );
    }

    #[test]
    fn undo_then_close_uses_the_replacement_endpoint() {
        let tokens = [
            "0", "0", "10", "0", "10", "10", "20", "20", "--undo", "0", "10", "--close",
        ]
        .map(String::from);
        let drawing = parse_drawing(&tokens).unwrap();

        assert_eq!(drawing.segments().len(), 4);
        assert_eq!(drawing.segments()[2].end(), Point2::new(0.0, 10.0).unwrap());
        assert_eq!(
            drawing.segments()[3].start(),
            Point2::new(0.0, 10.0).unwrap()
        );
        assert_eq!(drawing.segments()[3].end(), Point2::new(0.0, 0.0).unwrap());
    }

    #[test]
    fn undo_requires_a_segment_in_the_current_chain() {
        let tokens = ["0", "0", "--undo", "10", "0"].map(String::from);
        let error = parse_drawing(&tokens).unwrap_err();
        assert!(error.to_string().contains("chain 1 has no segment to undo"));
    }
}
