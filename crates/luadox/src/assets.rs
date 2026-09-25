//! The asset bundle, compiled in.
//!
//! Fourteen files ship in the Python's `luadox/data/`, and a fifteenth does not: the html
//! renderer reads `sidebar.tmpl.html` from the bundle when no `project.sidebar_template`
//! is configured, and no branch of the fork contains that file. A run without one dies in
//! the renderer's constructor with `FileNotFoundError`. The production config sets one, so nobody
//! has noticed. This ships a default, a deliberate deviation from the Python.

/// One file of the bundle, at the path the Python would list it under.
pub struct Asset {
    pub path: &'static str,
    pub bytes: &'static [u8],
}

/// Everything under `data/`, in the order `sorted(self.files)` produces.
///
/// The Python sorts host-separator paths (`img\i-left.svg` on Windows), which happens not
/// to change the order for this bundle: `/` and `\` both sort after `.` and before any
/// letter, and no two entries differ only there.
pub const BUNDLE: [Asset; 15] = [
    asset("foot.tmpl.html", include_bytes!("../assets/foot.tmpl.html")),
    asset("head.tmpl.html", include_bytes!("../assets/head.tmpl.html")),
    asset(
        "img/i-bitbucket.svg",
        include_bytes!("../assets/img/i-bitbucket.svg"),
    ),
    asset(
        "img/i-download.svg",
        include_bytes!("../assets/img/i-download.svg"),
    ),
    asset(
        "img/i-github.svg",
        include_bytes!("../assets/img/i-github.svg"),
    ),
    asset(
        "img/i-gitlab.svg",
        include_bytes!("../assets/img/i-gitlab.svg"),
    ),
    asset("img/i-left.svg", include_bytes!("../assets/img/i-left.svg")),
    asset(
        "img/i-right.svg",
        include_bytes!("../assets/img/i-right.svg"),
    ),
    asset(
        "js-search.min.js",
        include_bytes!("../assets/js-search.min.js"),
    ),
    asset("luadox.css", include_bytes!("../assets/luadox.css")),
    asset("prism.css", include_bytes!("../assets/prism.css")),
    asset("prism.js", include_bytes!("../assets/prism.js")),
    asset("search.js", include_bytes!("../assets/search.js")),
    asset(
        "search.tmpl.html",
        include_bytes!("../assets/search.tmpl.html"),
    ),
    asset(
        "sidebar.tmpl.html",
        include_bytes!("../assets/sidebar.tmpl.html"),
    ),
];

/// The files copied verbatim into the output, in the order the Python copies them.
pub const COPIED: [&str; 11] = [
    "luadox.css",
    "prism.css",
    "prism.js",
    "js-search.min.js",
    "search.js",
    "img/i-left.svg",
    "img/i-right.svg",
    "img/i-download.svg",
    "img/i-github.svg",
    "img/i-gitlab.svg",
    "img/i-bitbucket.svg",
];

const fn asset(path: &'static str, bytes: &'static [u8]) -> Asset {
    Asset { path, bytes }
}

pub fn get(path: &str) -> Option<&'static [u8]> {
    BUNDLE.iter().find(|a| a.path == path).map(|a| a.bytes)
}

pub fn text(path: &str) -> String {
    get(path)
        .map(|b| String::from_utf8_lossy(b).into_owned())
        .unwrap_or_default()
}

/// The cache-buster: the first seven hex digits of a sha256 over every bundle file,
/// concatenated in sorted-path order with no separators and no names.
///
/// Two implementations that ship the same assets still differ here if either re-encodes a
/// byte -- these are stored with LF where the Python's checkout has CRLF -- so spec/run.py
/// normalises `?<hex>` to a fixed token on both sides. Parity on the *value* is not
/// required; parity on *where it appears* is.
pub fn version() -> String {
    let mut hasher = Sha256::new();
    for asset in &BUNDLE {
        hasher.update(asset.bytes);
    }
    let digest = hasher.finalize();
    let mut hex = String::with_capacity(8);
    for byte in digest.iter().take(4) {
        hex.push_str(&format!("{byte:02x}"));
    }
    hex.truncate(7);
    hex
}

/// A small sha256.
///
/// `blake2` is already a dependency, for reference ids, but it is not sha256 and the
/// cache-buster has to be the same function the Python computes. This is cheaper than a
/// second digest crate for one four-byte value.
struct Sha256 {
    state: [u32; 8],
    buffer: Vec<u8>,
    length: u64,
}

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

impl Sha256 {
    fn new() -> Self {
        Self {
            state: [
                0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
                0x5be0cd19,
            ],
            buffer: Vec::with_capacity(64),
            length: 0,
        }
    }

    fn update(&mut self, data: &[u8]) {
        self.length = self.length.wrapping_add(data.len() as u64);
        for byte in data {
            self.buffer.push(*byte);
            if self.buffer.len() == 64 {
                let block = std::mem::take(&mut self.buffer);
                self.compress(&block);
            }
        }
    }

    fn finalize(mut self) -> [u8; 32] {
        let bits = self.length.wrapping_mul(8);
        self.update(&[0x80]);
        while self.buffer.len() != 56 {
            self.update(&[0]);
        }
        // The length is appended outside update(), so it does not count itself.
        let tail = bits.to_be_bytes();
        for byte in tail {
            self.buffer.push(byte);
        }
        let block = std::mem::take(&mut self.buffer);
        self.compress(&block);

        let mut out = [0u8; 32];
        for (i, word) in self.state.iter().enumerate() {
            for (j, byte) in word.to_be_bytes().iter().enumerate() {
                if let Some(slot) = out.get_mut(i * 4 + j) {
                    *slot = *byte;
                }
            }
        }
        out
    }

    fn compress(&mut self, block: &[u8]) {
        let mut w = [0u32; 64];
        for i in 0..16 {
            let b = block.get(i * 4..i * 4 + 4).unwrap_or(&[0, 0, 0, 0]);
            let word = u32::from_be_bytes([
                b.first().copied().unwrap_or(0),
                b.get(1).copied().unwrap_or(0),
                b.get(2).copied().unwrap_or(0),
                b.get(3).copied().unwrap_or(0),
            ]);
            if let Some(slot) = w.get_mut(i) {
                *slot = word;
            }
        }
        for i in 16..64 {
            let at = |n: usize| w.get(n).copied().unwrap_or(0);
            let (a, b) = (at(i - 15), at(i - 2));
            let s0 = a.rotate_right(7) ^ a.rotate_right(18) ^ (a >> 3);
            let s1 = b.rotate_right(17) ^ b.rotate_right(19) ^ (b >> 10);
            let value = at(i - 16)
                .wrapping_add(s0)
                .wrapping_add(at(i - 7))
                .wrapping_add(s1);
            if let Some(slot) = w.get_mut(i) {
                *slot = value;
            }
        }
        let mut h = self.state;
        for i in 0..64 {
            let at = |n: usize| h.get(n).copied().unwrap_or(0);
            let (a, b, c, d) = (at(0), at(1), at(2), at(3));
            let (e, f, g, hh) = (at(4), at(5), at(6), at(7));
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ ((!e) & g);
            let temp1 = hh
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(K.get(i).copied().unwrap_or(0))
                .wrapping_add(w.get(i).copied().unwrap_or(0));
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let temp2 = s0.wrapping_add(maj);
            h = [
                temp1.wrapping_add(temp2),
                a,
                b,
                c,
                d.wrapping_add(temp1),
                e,
                f,
                g,
            ];
        }
        for (i, value) in h.iter().enumerate() {
            if let Some(slot) = self.state.get_mut(i) {
                *slot = slot.wrapping_add(*value);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sha256_hex(data: &[u8]) -> String {
        let mut h = Sha256::new();
        h.update(data);
        h.finalize().iter().fold(String::new(), |mut acc, b| {
            use std::fmt::Write as _;
            let _ = write!(acc, "{b:02x}");
            acc
        })
    }

    #[test]
    fn sha256_matches_known_vectors() {
        assert_eq!(
            sha256_hex(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        // Longer than one block, so the buffering and the length field are exercised.
        assert_eq!(
            sha256_hex(&[b'a'; 200]),
            "c2a908d98f5df987ade41b5fce213067efbcc21ef2240212a41e54b5e7c28ae5"
        );
    }

    #[test]
    fn the_bundle_is_sorted_and_covers_what_is_copied() {
        let paths: Vec<&str> = BUNDLE.iter().map(|a| a.path).collect();
        let mut sorted = paths.clone();
        sorted.sort_unstable();
        assert_eq!(paths, sorted, "BUNDLE must be in sorted-path order");
        for name in COPIED {
            assert!(
                get(name).is_some(),
                "{name} is copied but not in the bundle"
            );
        }
    }

    #[test]
    fn the_version_is_seven_hex_digits() {
        let v = version();
        assert_eq!(v.len(), 7);
        assert!(v.chars().all(|c| c.is_ascii_hexdigit()), "{v}");
    }

    /// The templates the page frame interpolates have to be there, and the trailing
    /// newline of each is load-bearing: `head.tmpl.html` ends with one and the other two
    /// do not, which is what puts a blank line after `<body>` and none before `</body>`.
    #[test]
    fn the_templates_keep_their_trailing_newlines() {
        assert!(text("head.tmpl.html").ends_with(">\n"));
        assert!(!text("foot.tmpl.html").ends_with('\n'));
        assert!(!text("search.tmpl.html").ends_with('\n'));
        assert!(!text("sidebar.tmpl.html").ends_with('\n'));
    }
}
