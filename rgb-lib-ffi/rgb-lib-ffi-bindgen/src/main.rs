use anyhow;
use camino::Utf8PathBuf;
use std::fmt;
use std::path::PathBuf;
use std::str::FromStr;
use structopt::StructOpt;

#[derive(Debug, PartialEq)]
pub enum Language {
    Kotlin,
    Python,
    Swift,
}

impl fmt::Display for Language {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Language::Kotlin => write!(f, "kotlin"),
            Language::Python => write!(f, "python"),
            Language::Swift => write!(f, "swift"),
        }
    }
}

#[derive(Debug)]
pub enum Error {
    UnsupportedLanguage,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}", self)
    }
}

impl FromStr for Language {
    type Err = Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "kotlin" => Ok(Language::Kotlin),
            "python" => Ok(Language::Python),
            "swift" => Ok(Language::Swift),
            _ => Err(Error::UnsupportedLanguage),
        }
    }
}

fn generate_bindings(opt: &Opt) -> anyhow::Result<(), anyhow::Error> {
    uniffi_bindgen::generate_bindings(
        Utf8PathBuf::from_path_buf(opt.udl_file.clone())
            .expect("a valid path")
            .as_path(),
        None,
        vec![opt.language.to_string().as_str()],
        Some(
            Utf8PathBuf::from_path_buf(opt.out_dir.clone())
                .expect("a valid path")
                .as_path(),
        ),
        None,
        false,
    )?;

    Ok(())
}

#[derive(Debug, StructOpt)]
#[structopt(
    name = "rgb-lib-ffi-bindgen",
    about = "A tool to generate rgb-lib-ffi language bindings"
)]
struct Opt {
    /// UDL file
    #[structopt(env = "RGBFFI_BINDGEN_UDL", short, long, default_value("src/rgb-lib.udl"), parse(try_from_str = PathBuf::from_str))]
    udl_file: PathBuf,

    /// Language to generate bindings for
    #[structopt(env = "RGBFFI_BINDGEN_LANGUAGE", short, long, possible_values(&["kotlin","python","swift"]), parse(try_from_str = Language::from_str))]
    language: Language,

    /// Output directory to put generated language bindings
    #[structopt(env = "RGBFFI_BINDGEN_OUTPUT_DIR", short, long, parse(try_from_str = PathBuf::from_str))]
    out_dir: PathBuf,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let opt = Opt::from_args();

    println!("Input UDL file is {:?}", opt.udl_file);
    println!("Chosen language is {}", opt.language);
    println!("Output directory is {:?}", opt.out_dir);

    generate_bindings(&opt)?;

    Ok(())
}
