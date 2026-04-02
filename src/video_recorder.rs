//! Video recording module that pipes raw BGRA frames to FFmpeg for MP4 encoding.

use std::io::Write;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::time::Instant;

/// Manages an FFmpeg child process that receives raw BGRA pixel data via stdin
/// and encodes it into an MP4 video file.
pub struct VideoRecorder {
    ffmpeg_process: Child,
    stdin: Option<std::io::BufWriter<ChildStdin>>,
    frame_width: u32,
    frame_height: u32,
    frames_written: usize,
    total_frames: usize,
    start_instant: Instant,
}

impl VideoRecorder {
    /// Spawn an FFmpeg process configured to receive raw BGRA frames from stdin.
    ///
    /// # Arguments
    /// * `output_path` - Destination MP4 file path
    /// * `width` - Frame width in pixels
    /// * `height` - Frame height in pixels
    /// * `total_frames` - Estimated total number of frames (for progress reporting)
    pub fn new(
        output_path: &str,
        width: u32,
        height: u32,
        total_frames: usize,
    ) -> Result<Self, String> {
        // Verify ffmpeg is available
        let ffmpeg_check = Command::new("ffmpeg")
            .arg("-version")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map_err(|e| format!("ffmpeg not found in PATH: {}", e))?;

        if !ffmpeg_check.success() {
            return Err("ffmpeg is not available or returned an error".to_string());
        }

        let size_arg = format!("{}x{}", width, height);
        let ffmpeg_args = vec![
            "-y",
            "-f",
            "rawvideo",
            "-pix_fmt",
            "bgra",
            "-s",
            &size_arg,
            "-r",
            "60",
            "-i",
            "pipe:0",
            "-c:v",
            "libx264",
            "-preset",
            "fast",
            "-crf",
            "18",
            "-pix_fmt",
            "yuv420p",
            output_path,
        ];

        log::info!("Spawning ffmpeg with args: {:?}", ffmpeg_args);

        let mut cmd = Command::new("ffmpeg");
        cmd.args(&ffmpeg_args)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        #[cfg(target_os = "windows")]
        {
            use std::os::windows::process::CommandExt;
            cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
        }
        let mut process = cmd
            .spawn()
            .map_err(|e| format!("Failed to spawn ffmpeg: {}", e))?;

        let stdin = process
            .stdin
            .take()
            .ok_or_else(|| "Failed to open ffmpeg stdin".to_string())?;

        let stdin =
            std::io::BufWriter::with_capacity(width as usize * height as usize * 4 * 2, stdin);

        log::info!(
            "VideoRecorder started: {}x{} @ 60fps -> {}",
            width,
            height,
            output_path
        );

        Ok(Self {
            ffmpeg_process: process,
            stdin: Some(stdin),
            frame_width: width,
            frame_height: height,
            frames_written: 0,
            total_frames,
            start_instant: Instant::now(),
        })
    }

    /// Write a single frame of raw BGRA pixel data to the FFmpeg process.
    ///
    /// The data must be exactly `width * height * 4` bytes in BGRA order.
    pub fn write_frame(&mut self, bgra_data: &[u8]) -> Result<(), String> {
        let expected_size = self.frame_width as usize * self.frame_height as usize * 4;
        if bgra_data.len() != expected_size {
            return Err(format!(
                "Frame size mismatch: expected {} bytes, got {}",
                expected_size,
                bgra_data.len()
            ));
        }

        let stdin = self
            .stdin
            .as_mut()
            .ok_or_else(|| "ffmpeg stdin already closed".to_string())?;

        stdin
            .write_all(bgra_data)
            .map_err(|e| format!("Failed to write frame to ffmpeg stdin: {}", e))?;
        stdin
            .flush()
            .map_err(|e| format!("Failed to flush ffmpeg stdin: {}", e))?;

        self.frames_written += 1;
        Ok(())
    }

    /// Returns recording progress as a fraction from 0.0 to 1.0.
    pub fn progress(&self) -> f64 {
        if self.total_frames == 0 {
            return 0.0;
        }
        self.frames_written as f64 / self.total_frames as f64
    }

    /// Returns the number of frames written so far.
    pub fn frames_written(&self) -> usize {
        self.frames_written
    }

    /// Finish recording: close stdin, wait for FFmpeg to finish encoding, and
    /// return the total elapsed time.
    pub fn finish(&mut self) -> Result<f64, String> {
        log::info!(
            "Finalizing video... ({} frames written)",
            self.frames_written
        );

        // Close stdin to signal FFmpeg that input is complete
        drop(self.stdin.take());

        let elapsed = self.start_instant.elapsed();

        let status = self
            .ffmpeg_process
            .wait()
            .map_err(|e| format!("Failed to wait for ffmpeg process: {}", e))?;

        if status.success() {
            log::info!(
                "Video recording complete: {} frames in {:.1}s ({:.1} fps)",
                self.frames_written,
                elapsed.as_secs_f64(),
                self.frames_written as f64 / elapsed.as_secs_f64().max(0.001),
            );
            Ok(elapsed.as_secs_f64())
        } else {
            Err(format!("ffmpeg exited with status: {}", status))
        }
    }
}

impl Drop for VideoRecorder {
    fn drop(&mut self) {
        if self.stdin.is_some() {
            // Try to close stdin if still open
            drop(self.stdin.take());
            // Don't wait - just let the OS handle cleanup
        }
    }
}
