//! `pebrand show EXE` prints the version strings and manifest of an exe.
//! `pebrand brand IN OUT MANIFEST VERSION [ICON]` writes a copy of IN whose
//! version strings name Krunk (keeping the original copyright), whose
//! manifest is MANIFEST and, if given, whose main icon (resource group
//! `MAINICON`) is the multi-size .ico file ICON. Used on CEF's
//! `bootstrap.exe`, which becomes `Krunk.exe`.
use editpe::Image;
use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.iter().map(String::as_str).collect::<Vec<_>>().as_slice() {
        ["show", exe] => show(exe),
        ["brand", input, output, manifest, version] => brand(input, output, manifest, version, None),
        ["brand", input, output, manifest, version, icon] => brand(input, output, manifest, version, Some(icon)),
        _ => Err("usage: pebrand show EXE | pebrand brand IN OUT MANIFEST VERSION [ICON]".into()),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("pebrand: {e}");
            ExitCode::FAILURE
        }
    }
}

fn show(exe: &str) -> Result<(), String> {
    let image = Image::parse_file(exe).map_err(|e| format!("{exe}: {e:?}"))?;
    let res = image.resource_directory().ok_or("no resources")?;
    if let Some(v) = res.get_version_info().map_err(|e| format!("{e:?}"))? {
        println!("file version {:?}, product version {:?}", v.info.file_version, v.info.product_version);
        for table in &v.strings {
            for (k, val) in &table.strings {
                println!("{}: {k} = {val}", table.key);
            }
        }
    }
    println!("{}", res.get_manifest().map_err(|e| format!("{e:?}"))?.unwrap_or_default());
    Ok(())
}

fn brand(input: &str, output: &str, manifest: &str, version: &str, icon: Option<&str>) -> Result<(), String> {
    let mut image = Image::parse_file(input).map_err(|e| format!("{input}: {e:?}"))?;
    let mut res = image.resource_directory().cloned().ok_or("no resources")?;
    let manifest = std::fs::read_to_string(manifest).map_err(|e| format!("{manifest}: {e}"))?;
    let mut info = res.get_version_info().map_err(|e| format!("{e:?}"))?.ok_or("no version info")?;
    for table in &mut info.strings {
        let engine = table.strings.get("FileVersion").cloned().unwrap_or_default();
        for (key, value) in [
            ("CompanyName", "Krunk".to_string()),
            ("FileDescription", "Krunk web browser".to_string()),
            ("ProductName", "Krunk".to_string()),
            ("InternalName", "Krunk".to_string()),
            ("OriginalFilename", "Krunk.exe".to_string()),
            ("ProductVersion", version.to_string()),
            ("Comments", format!("Chromium Embedded Framework sandbox bootstrap {engine}")),
        ] {
            table.strings.insert(key.to_string(), value);
        }
    }
    res.set_version_info(&info).map_err(|e| format!("{e:?}"))?;
    res.set_manifest(&manifest).map_err(|e| format!("{e:?}"))?;
    if let Some(icon) = icon {
        res.set_main_icon_file(icon).map_err(|e| format!("{icon}: {e:?}"))?;
    }
    image.set_resource_directory(res).map_err(|e| format!("{e:?}"))?;
    image.write_file(output).map_err(|e| format!("{output}: {e:?}"))
}
