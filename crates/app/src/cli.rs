//! Command line: `--log-file <path>` (or `--log-file=<path>`). Unknown arguments are ignored: macOS
//! and Xcode add their own (`-psn_0_123`, `-NSDocumentRevisionsDebugMode`, ...).

use std::ffi::OsString;
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::path::PathBuf;

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Options {
    pub log_file: Option<PathBuf>,
    pub open_settings: bool,
    pub open_about: bool,
}

/// Parses the arguments after the program name. `Err` carries a message for the user.
pub fn parse(args: impl IntoIterator<Item = OsString>) -> Result<Options, String> {
    let mut options = Options::default();
    let mut args = args.into_iter();
    while let Some(arg) = args.next() {
        if arg == "--log-file" {
            let path = args
                .next()
                .filter(|path| !path.is_empty())
                .ok_or("--log-file needs a path")?;
            options.log_file = Some(PathBuf::from(path));
        } else if let Some(path) = arg.as_bytes().strip_prefix(b"--log-file=") {
            if path.is_empty() {
                return Err("--log-file needs a path".to_owned());
            }
            options.log_file = Some(PathBuf::from(OsString::from_vec(path.to_vec())));
        } else if arg == "--open-settings" || arg == "-s" {
            options.open_settings = true;
        } else if arg == "--open-about" || arg == "-a" {
            options.open_about = true;
        }
    }
    Ok(options)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_strs(args: &[&str]) -> Result<Options, String> {
        parse(args.iter().map(OsString::from))
    }

    #[test]
    fn no_arguments_means_no_log_file() {
        assert_eq!(parse_strs(&[]), Ok(Options::default()));
    }

    #[test]
    fn log_file_in_both_spellings() {
        let expected = Ok(Options {
            log_file: Some(PathBuf::from("/tmp/ls.log")),
            open_settings: false,
            open_about: false,
        });
        assert_eq!(parse_strs(&["--log-file", "/tmp/ls.log"]), expected);
        assert_eq!(parse_strs(&["--log-file=/tmp/ls.log"]), expected);
    }

    #[test]
    fn the_last_log_file_wins() {
        let options = parse_strs(&["--log-file", "a", "--log-file=b"]).unwrap();
        assert_eq!(options.log_file, Some(PathBuf::from("b")));
    }

    #[test]
    fn a_missing_or_empty_path_is_an_error() {
        assert!(parse_strs(&["--log-file"]).is_err());
        assert!(parse_strs(&["--log-file", ""]).is_err());
        assert!(parse_strs(&["--log-file="]).is_err());
    }

    #[test]
    fn unknown_arguments_are_ignored() {
        let options =
            parse_strs(&["-psn_0_12345", "-NSDocumentRevisionsDebugMode", "YES"]).unwrap();
        assert_eq!(options, Options::default());
    }

    #[test]
    fn paths_do_not_have_to_be_utf8() {
        let path = OsString::from_vec(vec![b'/', b't', 0xFF, b'x']);
        let options = parse([OsString::from("--log-file"), path.clone()]).unwrap();
        assert_eq!(options.log_file, Some(PathBuf::from(path)));
    }
}
