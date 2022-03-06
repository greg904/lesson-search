use std::env;
use std::fs;
use std::fs::File;
use std::io::BufRead;
use std::io::BufReader;
use std::path::Path;

fn parse_words(line: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut current = Vec::new();
    let mut quote = false;
    let mut escape = false;
    for c in line.as_bytes() {
        if escape {
            current.push(*c);
            escape = false;
        } else {
            if *c == b'"' {
                quote = !quote;
            } else if *c == b'\\' {
                escape = true;
            } else if *c == b' ' && !quote {
                if !current.is_empty() {
                    words.push(std::str::from_utf8(&current).unwrap().to_owned());
                    current.clear();
                }
            } else {
                current.push(*c);
            }
        }
    }
    if !current.is_empty() {
        words.push(String::from_utf8(current).unwrap());
    }
    words
}

fn main() {
    let out_dir = env::var_os("OUT_DIR").unwrap();
    let out_dir_path = Path::new(&out_dir);

    let mut code = "pub const SYNONYMS: &[(&str, &[&str])] = &[\n".to_owned();

    let reader = BufReader::new(File::open("src/synonyms.txt").unwrap());
    for line in reader.lines() {
        let synonyms = parse_words(&line.unwrap());
        if synonyms.len() < 2 {
            continue;
        }
        code += "    (\"";
        for c in synonyms[0].escape_default() {
            code.push(c);
        }
        code += "\", &[";
        let mut first = true;
        for s in synonyms.iter().skip(1) {
            if first {
                first = false;
            } else {
                code += ", ";
            }
            code += "\"";
            for c in s.escape_default() {
                code.push(c);
            }
            code += "\"";
        }
        code += "]),\n";
    }

    code += "];\n\n";

    fs::write(out_dir_path.join("synonyms.rs"), code).unwrap();

    println!("cargo:rerun-if-changed=src/synonyms.txt");
}
