use std::{collections::HashSet, io::Write as _, path::PathBuf};

use clap::Parser;
use subsetter::Profile;
use ttf_parser::Face;
use woff_convert::{convert_ttf_to_woff2, convert_woff2_to_ttf};

/// Simple program to greet a person
#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    /// The font file to subset
    input: PathBuf,
    /// The output file to write the subsetted font to. If not specified, the
    /// subsetted font is written to stdout
    #[arg(short, long)]
    output: Option<PathBuf>,
    /// The output format, either "woff2" or "ttf". If not specified, the output
    /// format is inferred from the output file extension
    #[arg(short, long)]
    format: Option<String>,

    /// The glyphs to subset
    #[arg(short, long, value_delimiter = ',', num_args = 1..)]
    glyphs: Option<Vec<u16>>,
    /// The characters to subset, as a string
    #[arg(short, long)]
    chars: Option<String>,
    /// Whether to map the glyphs to PUA codepoints
    #[arg(long, default_value = "false")]
    glyphs_to_pua: bool,
    /// Whether to subset all glyphs, in this case this tool acts as a simple
    /// format converter
    #[arg(long, short, conflicts_with_all = ["glyphs", "chars"], default_value = "false")]
    all: bool,
}

fn main() {
    let args = Args::parse();
    let mut font_data = std::fs::read(&args.input).expect("could not read font file");
    let initial_size = font_data.len();
    if args.input.extension().unwrap() == "woff2" {
        font_data =
            convert_woff2_to_ttf(&font_data).expect("could not convert WOFF2 to TTF");
    }
    let face = Face::parse(&font_data, 0).expect("could not parse font file");
    let mut glyphs: HashSet<u16> = HashSet::new();
    if let Some(g) = &args.glyphs {
        glyphs.extend(g.iter().copied());
    }
    if let Some(c) = &args.chars {
        for ch in c.chars() {
            if let Some(g) = face.glyph_index(ch) {
                glyphs.insert(g.0);
            }
        }
    }
    if args.all {
        glyphs.extend(0..face.number_of_glyphs());
    }
    let glyphs = glyphs.into_iter().collect::<Vec<_>>();
    let profile =
        if args.glyphs_to_pua { Profile::web(&glyphs) } else { Profile::pdf(&glyphs) };
    let mut result =
        subsetter::subset(&font_data, 0, profile).expect("could not subset font");
    if let Some(output) = args.output {
        let woff2 = match args.format.as_deref() {
            Some("woff2") => true,
            Some("ttf") => false,
            None => output.extension().unwrap() == "woff2",
            _ => panic!("unsupported format"),
        };
        if woff2 {
            result = convert_ttf_to_woff2(&result, 11)
                .expect("could not convert TTF to WOFF2");
        }
        std::fs::write(output, &result).expect("could not write subsetted font");
        println!(
            "subsetted from {initial_size} to {} bytes ({}%)",
            result.len(),
            100 * result.len() / initial_size
        );
    } else {
        if let Some("woff2") = args.format.as_deref() {
            result = convert_ttf_to_woff2(&result, 11)
                .expect("could not convert TTF to WOFF2");
        }
        std::io::stdout()
            .write_all(&result)
            .expect("could not write subsetted font");
    }
}
