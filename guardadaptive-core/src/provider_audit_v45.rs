//! v0.45 bounded, read-only PnP discovery audit.
//! PnP inventory strings and IsSigned are *claims*, not verified identity,
//! authenticated device driver ABI or permission to write.
use std::{fs, io, path::Path};

const MAX_BYTES: u64 = 131_072;
const MAX_ROWS: usize = 256;
const MAX_FIELD: usize = 512;
const HEADER: &str = "GA_PROVIDER_SCAN_V45|1";

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Summary {
    pub rows: usize,
    pub instance_id_hints: usize,
    pub unsigned_id_hints: usize,
    pub name_only_spoofs: usize,
    pub publisher_mismatch_hints: usize,
}

fn invalid(why: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, why)
}

pub fn evaluate(text: &str) -> io::Result<Summary> {
    let mut iter = text.lines();
    if iter.next() != Some(HEADER) {
        return Err(invalid("invalid or missing PnP inventory schema"));
    }
    let mut r = Summary::default();
    for line in iter {
        if r.rows >= MAX_ROWS {
            return Err(invalid("too many candidate records"));
        }
        if line.is_empty() || line.ends_with('\r') {
            return Err(invalid("empty record or CRLF in strict inventory"));
        }
        let fields: Vec<&str> = line.split('|').collect();
        if fields.len() != 6
            || fields
                .iter()
                .any(|f| f.len() > MAX_FIELD || f.chars().any(char::is_control))
            || fields[0].is_empty()
            || fields[1].is_empty()
        {
            return Err(invalid("malformed inventory record"));
        }
        if fields[5] != "True" && fields[5] != "False" {
            return Err(invalid("invalid IsSigned claim"));
        }
        r.rows += 1;
        let id = fields[0].to_ascii_uppercase();
        let name = fields[1].to_ascii_uppercase();
        let vendor = fields[2].to_ascii_uppercase();
        let id_hint = id.starts_with("ROOT\\VIGEMBUS\\");
        let name_hint = [
            "VIGEM",
            "VJOY",
            "VIRTUAL GAMEPAD",
            "VIRTUAL CONTROLLER",
            "XBOX",
            "XUSB",
        ]
        .iter()
        .any(|word| name.contains(word));
        if id_hint {
            r.instance_id_hints += 1;
            if fields[5] != "True" {
                r.unsigned_id_hints += 1;
            }
            // Manufacturer text is also untrusted. A match is NOT verification.
            if !vendor.contains("NEFARIUS") {
                r.publisher_mismatch_hints += 1;
            }
        } else if name_hint {
            r.name_only_spoofs += 1;
        }
    }
    Ok(r)
}

pub fn run(input: &Path, out: &Path) -> io::Result<()> {
    let meta = fs::symlink_metadata(input)?;
    if !meta.file_type().is_file() || meta.len() > MAX_BYTES {
        return Err(invalid("source must be an ordinary bounded inventory file"));
    }
    let data = fs::read_to_string(input)?;
    let r = evaluate(&data)?;
    // Summary excludes untrusted input strings. Even seemingly signed rows
    // do not authorize real driver I/O or attest to vendor API compatibility.
    let report = format!(
        concat!(
            "{{\"schema\":1,\"source\":\"v45_pnp_readonly_provider_audit\",",
            "\"inventory_rows\":{},\"pnp_id_hints\":{},",
            "\"unsigned_id_hints\":{},\"name_only_spoof_suspicions\":{},",
            "\"publisher_mismatch_hints\":{},",
            "\"pnp_metadata_is_not_authentication\":true,",
            "\"os_signature_claim_not_cryptographic_attestation\":true,",
            "\"compatible_driver_abi_verified\":false,",
            "\"driver_authorized\":false,\"virtual_output_enabled\":false,",
            "\"driver_installs\":0,\"driver_writes\":0,\"game_input_writes\":0,",
            "\"production_gate\":\"BLOCKED\"}}\n"
        ),
        r.rows,
        r.instance_id_hints,
        r.unsigned_id_hints,
        r.name_only_spoofs,
        r.publisher_mismatch_hints
    );
    crate::truth_lab::save_new(out, &report)?;
    println!(
        "V045: {} read-only PnP observations; {} candidate ID hints; all output BLOCKED",
        r.rows, r.instance_id_hints
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    const HEAD: &str = "GA_PROVIDER_SCAN_V45|1\n";
    #[test]
    fn signed_claim_and_exact_id_cannot_authorize_driver() {
        let data = format!(
            "{HEAD}ROOT\\ViGEmBus\\0000|ViGEmBus Device|Nefarius Software Solutions e.U.|oem32.inf|Microsoft Windows Hardware Compatibility Publisher|True\n"
        );
        assert_eq!(
            evaluate(&data).unwrap(),
            Summary {
                rows: 1,
                instance_id_hints: 1,
                unsigned_id_hints: 0,
                name_only_spoofs: 0,
                publisher_mismatch_hints: 0
            }
        );
    }
    #[test]
    fn spoofed_friendly_name_and_unsigned_device_are_flagged() {
        let data = format!(
            "{HEAD}ROOT\\FAKE\\0001|ViGEmBus Device|Nefarius Software Solutions e.U.|oem9.inf|Microsoft|True\nROOT\\ViGEmBus\\0001|Gamepad|Faux Provider|oem7.inf||False\n"
        );
        let a = evaluate(&data).unwrap();
        assert_eq!(a.rows, 2);
        assert_eq!(a.name_only_spoofs, 1);
        assert_eq!(a.unsigned_id_hints, 1);
        assert_eq!(a.publisher_mismatch_hints, 1);
    }
    #[test]
    fn malformed_claims_and_excess_inventory_are_rejected() {
        for data in [
            "",
            "GA_PROVIDER_SCAN_V44|1\n",
            "GA_PROVIDER_SCAN_V45|1\nmalformed\n",
            "GA_PROVIDER_SCAN_V45|1\nx|y|z|inf|signer|YES\n",
            "GA_PROVIDER_SCAN_V45|1\nx|y|z|inf|signer|True|extra\n",
            "GA_PROVIDER_SCAN_V45|1\nx|y\0|z|inf|signer|True\n",
        ] {
            assert!(evaluate(data).is_err(), "accepted invalid input {data:?}");
        }
        let too_many = format!("{HEAD}{}", "id|name|vendor|inf|signer|True\n".repeat(257));
        assert!(evaluate(&too_many).is_err());
    }
    #[test]
    fn output_denies_hardware_and_is_not_overwritten() {
        let root = std::env::temp_dir().join(format!("ga-v45-{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        let source = root.join("candidate-inventory.txt");
        let target = root.join("candidate-result.json");
        let _ = fs::remove_file(&target);
        fs::write(&source, HEAD).unwrap();
        run(&source, &target).unwrap();
        let content = fs::read_to_string(&target).unwrap();
        assert!(content.contains("\"driver_authorized\":false"));
        assert!(content.contains("\"driver_writes\":0"));
        assert!(run(&source, &target).is_err());
        assert_eq!(fs::read_to_string(&target).unwrap(), content);
        fs::remove_dir_all(&root).unwrap();
    }
}
