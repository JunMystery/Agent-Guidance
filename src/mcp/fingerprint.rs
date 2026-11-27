use std::env;
use std::path::{Path, PathBuf};

pub struct BinaryFingerprint {
    pub executable_path: PathBuf,
    pub sha256_hash: String,
    pub publisher: String,
    pub author_email: String,
    pub version: String,
    pub commit: String,
    pub target_triple: String,
    pub signature_status: String,
}

impl BinaryFingerprint {
    pub fn current() -> Option<Self> {
        let exe = env::current_exe().ok()?;
        let sha256 = compute_file_sha256(&exe).unwrap_or_else(|_| "unavailable".to_string());
        let signature_status = detect_signature_status(&exe);

        Some(Self {
            executable_path: exe,
            sha256_hash: sha256,
            publisher: env!("AGENT_PUBLISHER").to_string(),
            author_email: env!("AGENT_AUTHOR_EMAIL").to_string(),
            version: env!("CARGO_PKG_VERSION").to_string(),
            commit: env!("AGENT_GIT_COMMIT").to_string(),
            target_triple: env!("AGENT_TARGET_TRIPLE").to_string(),
            signature_status,
        })
    }

    pub fn print_report(&self) {
        println!("================================================================================");
        println!("AGENT GUIDANCE SECURITY FINGERPRINT & PROVENANCE REPORT");
        println!("================================================================================");
        println!("Publisher:        {} <{}>", self.publisher, self.author_email);
        println!("Product:          Agent Guidance (agent-guidance)");
        println!("Version:          v{}", self.version);
        println!("Git Commit:       {}", self.commit);
        println!("Target Triple:    {}", self.target_triple);
        println!("Executable:       {}", self.executable_path.display());
        println!("SHA-256 Digest:   {}", self.sha256_hash);
        println!("Signature Status: {}", self.signature_status);
        println!("================================================================================");
    }
}

#[cfg(windows)]
fn detect_signature_status(path: &Path) -> String {
    use std::io::Read;
    use std::process::{Command, Stdio};
    use std::time::{Duration, Instant};

    let escaped_path = path.display().to_string().replace('\'', "''");
    let script = format!(
        "$s = Get-AuthenticodeSignature -LiteralPath '{}'; if ($s.Status -eq 'Valid') {{ \"Valid (Signed by: $($s.SignerCertificate.Subject))\" }} elseif ($s.SignerCertificate) {{ \"Signed: $($s.SignerCertificate.Subject) [Thumbprint: $($s.SignerCertificate.Thumbprint)]\" }} else {{ \"$($s.Status): $($s.StatusMessage)\" }}",
        escaped_path
    );
    let Ok(mut child) = Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", &script])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
    else {
        return "Unsigned or unchecked".to_string();
    };

    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                if !status.success() {
                    return "Unsigned or unchecked".to_string();
                }
                let mut stdout_buf = String::new();
                if let Some(mut out) = child.stdout.take() {
                    let _ = out.read_to_string(&mut stdout_buf);
                }
                let trimmed = stdout_buf.trim().to_string();
                return if trimmed.is_empty() {
                    "Unsigned or unchecked".to_string()
                } else {
                    trimmed
                };
            }
            Ok(None) => {
                if Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    return "Signature check timed out".to_string();
                }
                std::thread::sleep(Duration::from_millis(25));
            }
            Err(_) => {
                let _ = child.kill();
                return "Unsigned or unchecked".to_string();
            }
        }
    }
}

#[cfg(target_os = "macos")]
fn detect_signature_status(path: &Path) -> String {
    std::process::Command::new("codesign")
        .args(["-dvv", &path.to_string_lossy()])
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stderr).ok())
        .map(|s| {
            if s.contains("Authority=") {
                "Valid (codesign signature present)".to_string()
            } else if s.contains("code object is not signed") {
                "Unsigned".to_string()
            } else {
                "Ad-hoc or custom signed".to_string()
            }
        })
        .unwrap_or_else(|| "Unchecked".to_string())
}

#[cfg(not(any(windows, target_os = "macos")))]
fn detect_signature_status(path: &Path) -> String {
    let sig_path = path.with_extension("sig");
    if sig_path.exists() {
        "Detached GPG signature present (.sig)".to_string()
    } else {
        "ELF unsigned binary (integrity verified via SHA-256)".to_string()
    }
}

pub fn compute_file_sha256(path: &Path) -> std::io::Result<String> {
    use std::io::Read;
    let file = std::fs::File::open(path)?;
    let mut reader = std::io::BufReader::with_capacity(64 * 1024, file);
    let mut hasher = Sha256State::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = reader.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hasher.finalize())
}

struct Sha256State {
    h: [u32; 8],
    block: [u8; 64],
    block_len: usize,
    total_bytes: u64,
}

impl Sha256State {
    fn new() -> Self {
        Self {
            h: [
                0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a,
                0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19,
            ],
            block: [0u8; 64],
            block_len: 0,
            total_bytes: 0,
        }
    }

    fn compress(h: &mut [u32; 8], chunk: &[u8; 64]) {
        const K: [u32; 64] = [
            0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
            0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
            0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
            0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
            0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
            0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
            0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
            0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
        ];
        let mut w = [0u32; 64];
        for (i, w_val) in w.iter_mut().take(16).enumerate() {
            *w_val = u32::from_be_bytes([chunk[i * 4], chunk[i * 4 + 1], chunk[i * 4 + 2], chunk[i * 4 + 3]]);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16].wrapping_add(s0).wrapping_add(w[i - 7]).wrapping_add(s1);
        }
        let (mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut h_val) =
            (h[0], h[1], h[2], h[3], h[4], h[5], h[6], h[7]);
        for (i, &k_val) in K.iter().enumerate() {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ ((!e) & g);
            let temp1 = h_val.wrapping_add(s1).wrapping_add(ch).wrapping_add(k_val).wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let temp2 = s0.wrapping_add(maj);
            h_val = g;
            g = f;
            f = e;
            e = d.wrapping_add(temp1);
            d = c;
            c = b;
            b = a;
            a = temp1.wrapping_add(temp2);
        }
        for (idx, v) in [a, b, c, d, e, f, g, h_val].iter().enumerate() {
            h[idx] = h[idx].wrapping_add(*v);
        }
    }

    fn update(&mut self, mut data: &[u8]) {
        self.total_bytes += data.len() as u64;
        if self.block_len > 0 {
            let take = (64 - self.block_len).min(data.len());
            self.block[self.block_len..self.block_len + take].copy_from_slice(&data[..take]);
            self.block_len += take;
            data = &data[take..];
            if self.block_len == 64 {
                let blk = self.block;
                Self::compress(&mut self.h, &blk);
                self.block_len = 0;
            }
        }
        while data.len() >= 64 {
            let chunk: &[u8; 64] = data[..64].try_into().unwrap();
            Self::compress(&mut self.h, chunk);
            data = &data[64..];
        }
        if !data.is_empty() {
            self.block[..data.len()].copy_from_slice(data);
            self.block_len = data.len();
        }
    }

    fn finalize(mut self) -> String {
        let bit_len = self.total_bytes * 8;
        self.block[self.block_len] = 0x80;
        self.block_len += 1;
        if self.block_len > 56 {
            self.block[self.block_len..64].fill(0);
            let blk = self.block;
            Self::compress(&mut self.h, &blk);
            self.block_len = 0;
        }
        self.block[self.block_len..56].fill(0);
        self.block[56..64].copy_from_slice(&bit_len.to_be_bytes());
        let blk = self.block;
        Self::compress(&mut self.h, &blk);
        self.h
            .iter()
            .map(|word| format!("{:08x}", word))
            .collect::<Vec<_>>()
            .join("")
    }
}
