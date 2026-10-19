//! A smaller local MP4, published only after encoding succeeds and saves bytes.
use std::path::Path;
use std::process::Stdio;

pub const SCALE: &str = "scale=w='min(1280,iw)':h='min(720,ih)':force_original_aspect_ratio=decrease:force_divisible_by=2";

pub async fn compress(ffmpeg: &Path, input: &Path) -> Result<serde_json::Value, String> {
    if !input.is_file() {
        return Err("Elegí un archivo de video local.".into());
    }
    let before = std::fs::metadata(input).map_err(|e| e.to_string())?.len();
    let parent = input.parent().ok_or("Carpeta de origen inválida")?;
    let temp = tempfile::Builder::new()
        .prefix(".oval-compress-")
        .suffix(".mp4")
        .tempfile_in(parent)
        .map_err(|e| format!("No se puede escribir junto al video: {e}"))?;
    let mut cmd = tokio::process::Command::new(ffmpeg);
    cmd.args([
        "-nostdin",
        "-hide_banner",
        "-loglevel",
        "error",
        "-y",
        "-protocol_whitelist",
        "file,pipe,crypto,data",
        "-i",
    ])
    .arg(input)
    .args([
        "-map",
        "0:v:0",
        "-map",
        "0:a:0?",
        "-vf",
        SCALE,
        "-c:v",
        "libx264",
        "-crf",
        "28",
        "-preset",
        "fast",
        "-pix_fmt",
        "yuv420p",
        "-c:a",
        "aac",
        "-b:a",
        "96k",
        "-sn",
        "-dn",
        "-map_metadata",
        "-1",
        "-movflags",
        "+faststart",
    ])
    .arg(temp.path())
    .stdin(Stdio::null())
    .stdout(Stdio::null())
    .stderr(Stdio::piped())
    .kill_on_drop(true);
    crate::platform::prepare(&mut cmd);
    let output = cmd
        .output()
        .await
        .map_err(|e| format!("No se pudo iniciar FFmpeg: {e}"))?;
    if !output.status.success() {
        return Err("No se pudo comprimir ese video. Comprobá que sea un archivo válido y que FFmpeg admita su formato.".into());
    }
    let after = temp.as_file().metadata().map_err(|e| e.to_string())?.len();
    if after == 0 || after >= before {
        return Err("El video ya es liviano: no se creó otra copia porque no ahorraba espacio. El original sigue intacto.".into());
    }
    let stem = input.file_stem().unwrap_or_default().to_string_lossy();
    let mut temp = temp;
    for index in 0..1000 {
        let suffix = if index == 0 {
            String::new()
        } else {
            format!(" ({index})")
        };
        let path = parent.join(format!("{stem}-liviano{suffix}.mp4"));
        match temp.persist_noclobber(&path) {
            Ok(_) => {
                return Ok(
                    serde_json::json!({"path": path, "originalBytes": before, "outputBytes": after,
                "savedPercent": (1.0 - after as f64 / before as f64) * 100.0}),
                )
            }
            Err(error) if error.error.kind() == std::io::ErrorKind::AlreadyExists => {
                temp = error.file
            }
            Err(error) => return Err(format!("No se pudo guardar el video: {}", error.error)),
        }
    }
    Err("No se encontró un nombre libre para la copia.".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn real_compression_preserves_original_and_existing_output() {
        let dir = tempfile::tempdir().unwrap();
        let input = dir.path().join("video con espacios ñ.mkv");
        let result = std::process::Command::new("ffmpeg")
            .args([
                "-v",
                "error",
                "-f",
                "lavfi",
                "-i",
                "testsrc2=size=640x360:rate=12",
                "-t",
                "1",
                "-c:v",
                "ffv1",
            ])
            .arg(&input)
            .output()
            .expect("FFmpeg must be available for the compression test");
        assert!(result.status.success());
        let original = std::fs::read(&input).unwrap();
        let occupied = dir.path().join("video con espacios ñ-liviano.mp4");
        std::fs::write(&occupied, b"keep").unwrap();
        let result = compress(Path::new("ffmpeg"), &input).await.unwrap();
        let path = Path::new(result["path"].as_str().unwrap());
        assert!(path.ends_with("video con espacios ñ-liviano (1).mp4"));
        assert!(std::fs::metadata(path).unwrap().len() < original.len() as u64);
        assert_eq!(std::fs::read(&input).unwrap(), original);
        assert_eq!(std::fs::read(&occupied).unwrap(), b"keep");
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 3);
    }

    #[tokio::test]
    async fn invalid_input_leaves_no_temporary_file() {
        let dir = tempfile::tempdir().unwrap();
        let input = dir.path().join("invalid.mp4");
        std::fs::write(&input, b"invalid").unwrap();
        assert!(compress(Path::new("ffmpeg"), &input).await.is_err());
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
    }
}
