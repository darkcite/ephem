// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 Anton (darkcite)
//! Writes a sample channel for the interop check (`checks/channel_interop.mjs`): the IPNS name,
//! the signed record and a CAR of all blocks, into the directory given as the argument.

use ephem_channel::{Channel, car};

fn main() {
    let dir = std::env::args().nth(1).expect("output directory");
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs();
    let mut c = Channel::new(&[4; 32], "Interop", "Checked by the JavaScript IPFS libraries", now).unwrap();
    for i in 1..=70 {
        c.post(&format!("post {i} ✓"), 0, now).unwrap();
    }
    c.delete(2).unwrap();
    let (root, blocks) = c.build(now);
    let record = c.record(&root, now);
    std::fs::write(format!("{dir}/name.txt"), c.name().to_text()).unwrap();
    std::fs::write(format!("{dir}/root.txt"), root.to_text()).unwrap();
    std::fs::write(format!("{dir}/record.bin"), record).unwrap();
    std::fs::write(format!("{dir}/channel.car"), car::write(&[root], &blocks)).unwrap();
}
