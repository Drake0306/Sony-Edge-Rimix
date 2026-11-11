//! Text output for the camera's state and card.

use sony_ptpip::{CardKind, CardObject, PropertySummary};

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
    table(&rows, " | ")
}

/// Rows padded to their columns' widths and joined by `sep`.
fn table<const N: usize>(rows: &[[String; N]], sep: &str) -> String {
    let mut widths = [0usize; N];
    for row in rows {
        for (w, cell) in widths.iter_mut().zip(row) {
            *w = (*w).max(cell.chars().count());
        }
    }
    let mut out = String::new();
    for row in rows {
        let cells: Vec<String> = row
            .iter()
            .zip(widths)
            .map(|(cell, w)| format!("{cell:<w$}"))
            .collect();
        out.push_str(cells.join(sep).trim_end());
        out.push('\n');
    }
    out
}

/// A size as the card browser shows it, in binary units: `512 B`, `2.0 MB`,
/// `64 MB`, `1.2 GB` (one decimal under 10).
pub fn size_text(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["KB", "MB", "GB", "TB"];
    if bytes < 1024 {
        return format!("{bytes} B");
    }
    let mut value = bytes as f64 / 1024.0;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if value < 9.95 {
        format!("{value:.1} {}", UNITS[unit])
    } else {
        format!("{value:.0} {}", UNITS[unit])
    }
}

/// The size column: [`size_text`], or `4 GB+` when the camera cannot say.
pub fn card_size(obj: &CardObject) -> String {
    if obj.size_known {
        size_text(obj.bytes)
    } else {
        "4 GB+".into()
    }
}

pub fn kind_text(kind: CardKind) -> &'static str {
    match kind {
        CardKind::Still => "Still",
        CardKind::Raw => "Raw",
        CardKind::Video => "Video",
        CardKind::Other => "Other",
    }
}

/// The card newest first, by capture date then by handle; undated files last.
pub fn sort_card(card: &mut [CardObject]) {
    card.sort_by(|a, b| (b.captured, b.handle).cmp(&(a.captured, a.handle)));
}

/// The `card list` table: a header and one row per file, columns padded and
/// two spaces apart.
pub fn card_table(card: &[CardObject]) -> String {
    let mut rows: Vec<[String; 5]> = vec![[
        "Kind".into(),
        "Size".into(),
        "Date".into(),
        "Handle".into(),
        "Name".into(),
    ]];
    for o in card {
        rows.push([
            kind_text(o.kind).into(),
            card_size(o),
            o.captured
                .map_or_else(|| "-".into(), |d| d.format("%Y-%m-%d %H:%M").to_string()),
            o.handle.to_string(),
            o.filename.clone(),
        ]);
    }
    table(&rows, "  ")
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
            range: None,
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

    #[test]
    fn sizes_read_like_the_card_browser() {
        assert_eq!(size_text(512), "512 B");
        assert_eq!(size_text(2 << 20), "2.0 MB");
        assert_eq!(size_text(64 << 20), "64 MB");
        assert_eq!(size_text(1288 << 20), "1.3 GB");
        assert_eq!(size_text((12 << 30) / 10), "1.2 GB");
        assert_eq!(size_text(1536), "1.5 KB");
    }
}
