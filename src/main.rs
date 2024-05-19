use std::path::Path;

use subsetter::Profile;
use ttf_parser::{Face, GlyphId};

fn main() {
    let args = std::env::args().collect::<Vec<String>>();
    let font_path = args.get(1).expect("please provide a font path");
    let font_path = Path::new(font_path);
    let font_data = std::fs::read(font_path).expect("could not read font file");
    let face = Face::parse(&font_data, 0).expect("could not parse font file");
    let glyphs = (0..face.number_of_glyphs()).collect::<Vec<_>>();

    let profile = Profile::web(&glyphs);
    println!("found {} glyphs", face.number_of_glyphs());
    let result =
        subsetter::subset(&font_data, 0, profile).expect("could not subset font");
    let face = Face::parse(&result, 0).expect("could not parse subsetted font");
    for i in 0..face.number_of_glyphs() {
        let ch = char::from_u32(0xF0000 + i as u32).unwrap();
        assert_eq!(face.glyph_index(ch), Some(GlyphId(i)));
    }

    // std::fs::write("subset.ttf", &result).expect("could not write subsetted font");
    println!("subsetted to {} bytes", result.len());
}
