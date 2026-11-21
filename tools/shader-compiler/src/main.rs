//! Translate a cooked shader, optionally linking its outputs to a pixel shader.
use std::{env, fs, path::PathBuf};
use tuxbox_xshader::{extract, hlsl, meta, Stage};

fn run() -> Result<(), String> {
    let mut args = env::args_os().skip(1);
    let source = args.next().ok_or("expected a cooked shader path")?;
    let mut output = None;
    let mut profile_output = None;
    let mut consumer = None;
    let mut pixel_order = None;
    while let Some(argument) = args.next() {
        let slot = match argument.to_str() {
            Some("--hlsl-out") => &mut output,
            Some("--profile-out") => &mut profile_output,
            Some("--link-pixel") => &mut consumer,
            Some("--pixel-order") => &mut pixel_order,
            _ => return Err(format!("unknown option {argument:?}")),
        };
        if slot
            .replace(PathBuf::from(args.next().ok_or("missing path")?))
            .is_some()
        {
            return Err(format!("duplicate option {argument:?}"));
        }
    }
    let output = output.ok_or("--hlsl-out is required")?;
    let read =
        |path: &std::path::Path| fs::read(path).map_err(|e| format!("{}: {e}", path.display()));
    let shader = extract(&read(source.as_ref())?)?;
    let translated = match consumer {
        Some(path) => {
            let pixel = extract(&read(&path)?)?;
            if !matches!(shader.stage, Stage::Vertex | Stage::Geometry)
                || pixel.stage != Stage::Pixel
            {
                return Err("expected a vertex or geometry producer and a pixel consumer".into());
            }
            let pixel = hlsl::translate(&pixel)?;
            let order = pixel_order
                .as_deref()
                .map(read)
                .transpose()?
                .map(|dxil| meta::pixel_input_order(&dxil))
                .unwrap_or_default();
            hlsl::translate_ordered(&shader, Some(&pixel.ps_input_interp), &order)?
        }
        None if pixel_order.is_some() => return Err("--pixel-order requires --link-pixel".into()),
        None => hlsl::translate(&shader)?,
    };
    if let Some(path) = profile_output {
        let stage = match shader.stage {
            Stage::Pixel => "ps",
            Stage::Vertex => "vs",
            Stage::Geometry => "gs",
            Stage::Compute => "cs",
            _ => return Err("unsupported DXC stage".into()),
        };
        let model = if translated.required_wave_size.is_some() || translated.typed_int64_atomics {
            "6_6"
        } else if translated.needs_barycentrics {
            "6_1"
        } else {
            "6_0"
        };
        fs::write(&path, format!("{stage}_{model}"))
            .map_err(|e| format!("{}: {e}", path.display()))?;
    }
    let mut instructions = 0;
    let mut consumed = 0;
    let mut ended = false;
    for ins in tuxbox_xshader::rdna2::instructions(&shader.isa) {
        instructions += 1;
        consumed += ins.dwords * 4;
        ended |= ins.mnemonic == "s_endpgm";
    }
    println!("entry={:?}", shader.entry);
    println!("decode: {instructions} instrs, consumed {consumed}/{} bytes, terminated_on_s_endpgm={ended}", shader.isa.len());
    fs::write(&output, translated.hlsl).map_err(|e| format!("{}: {e}", output.display()))
}

fn main() -> std::process::ExitCode {
    match run() {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            std::process::ExitCode::FAILURE
        }
    }
}
