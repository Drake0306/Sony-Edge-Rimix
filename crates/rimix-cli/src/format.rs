//! Text output for the camera's state.

use sony_ptpip::PropertySummary;

/// The ten properties the desktop dock shows, in its order.
pub const DOCK_PROPS: [u16; 10] = [
    0x500E, 0xD20D, 0x5007, 0xD21E, 0x5010, 0x5005, 0x500A, 0x5013, 0x5004, 0xD20F,
];

/// Names of properties whose label starts with the value, as `ISO 1600`.
const KNOWN_NAMES: [(u16, &str); 1] = [(0xD21E, "ISO")];

/// A name for the Settings column: the label without its value when the
/// label starts with a known name, else the hex code.
pub fn setting_name(prop: &PropertySummary) -> String {
    for (code, name) in KNOWN_NAMES {
        if prop.code == code && prop.label.starts_with(name) {
            return name.to_string();
        }
    }
    hex_code(prop.code)
}

pub fn hex_code(code: u16) -> String {
    format!("0x{code:04X}")
}

/// The `props` table: a header and one row per property, columns padded.
pub fn props_table(props: &[&PropertySummary]) -> String {
    let mut rows: Vec<[String; 5]> = vec![[
        "Code".into(),
        "Setting".into(),
        "Value".into(),
        "Writable".into(),
        "Choices".into(),
    ]];
    for p in props {
        rows.push([
            hex_code(p.code),
            setting_name(p),
            p.short.clone(),
            if p.writable { "yes" } else { "no" }.into(),
            p.choices.len().to_string(),
        ]);
    }
    let mut widths = [0usize; 5];
    for row in &rows {
        for (w, cell) in widths.iter_mut().zip(row) {
            *w = (*w).max(cell.chars().count());
        }
    }
    let mut out = String::new();
    for row in &rows {
        let cells: Vec<String> = row
            .iter()
            .zip(widths)
            .map(|(cell, w)| format!("{cell:<w$}"))
            .collect();
        out.push_str(cells.join(" | ").trim_end());
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use sony_ptpip::ptp::PtpValue;
    use sony_ptpip::sony::property::Enabled;

    fn prop(code: u16, label: &str, short: &str) -> PropertySummary {
        PropertySummary {
            code,
            label: label.into(),
            short: short.into(),
            raw: PtpValue::U16(0),
            writable: true,
            enabled: Enabled::On,
            choices: Vec::new(),
        }
    }

    #[test]
    fn names_iso_and_falls_back_to_hex() {
        assert_eq!(setting_name(&prop(0xD21E, "ISO 1600", "1600")), "ISO");
        assert_eq!(setting_name(&prop(0xD20D, "1/125", "1/125")), "0xD20D");
    }

    #[test]
    fn table_has_a_header_and_aligned_rows() {
        let a = prop(0xD21E, "ISO 1600", "1600");
        let table = props_table(&[&a]);
        let lines: Vec<&str> = table.lines().collect();
        assert_eq!(lines[0], "Code   | Setting | Value | Writable | Choices");
        assert_eq!(lines[1], "0xD21E | ISO     | 1600  | yes      | 0");
    }
}
