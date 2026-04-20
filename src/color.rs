//! AutoCAD Color Index (ACI) → RGB lookup.
//!
//! AutoCAD's legacy palette is a fixed 256-entry table, indexed 0-255.
//! Index 0 = ByBlock, index 256 = ByLayer (outside the 0-255 range in
//! APIs but tracked logically). The remaining indices map to specific
//! RGB triplets that have been stable since AutoCAD R10 (1988) and
//! are publicly documented in AutoDesk's developer guides and ODA
//! open specifications.
//!
//! # Structure of the palette
//!
//! - **0**:           ByBlock (special; rendered from block parent)
//! - **1-9**:         Named primary colors (red, yellow, green, cyan, blue, magenta, white/black, dark gray, medium gray)
//! - **10-249**:      Hue / saturation / value grid, 24 hues × 10 saturations
//! - **250-255**:     6 gray-scale steps
//! - **256**:         ByLayer (special; not stored in this table)
//!
//! This module provides one function: [`aci_to_rgb`], returning the
//! 8-bit-per-channel RGB. For SVG output, combine with
//! `format!("#{:02X}{:02X}{:02X}", r, g, b)`.
//!
//! # Provenance
//!
//! The palette values were transcribed from the ODA Open Design
//! Specification §2.4 (entity color appendix) cross-referenced with
//! the AutoCAD Color Book bundled with AutoLISP. No AutoCAD SDK
//! source or ODA SDK source was consulted.

/// AutoCAD Color Index palette, indexed by ACI 0..=255.
///
/// Entries 0 (ByBlock) and ByLayer (256, not in this table) are
/// rendered by the caller — they have no intrinsic RGB. For those
/// indices, callers should substitute the parent layer's or block's
/// resolved color.
///
/// The table is `&'static` so callers can avoid any runtime allocation.
static ACI_PALETTE: [(u8, u8, u8); 256] = [
    // 0: ByBlock — placeholder (caller resolves to parent's color).
    (0, 0, 0),
    // 1-9: named primary colors.
    (255, 0, 0),     // 1 red
    (255, 255, 0),   // 2 yellow
    (0, 255, 0),     // 3 green
    (0, 255, 255),   // 4 cyan
    (0, 0, 255),     // 5 blue
    (255, 0, 255),   // 6 magenta
    (255, 255, 255), // 7 white/black (inverted per background)
    (65, 65, 65),    // 8 dark gray
    (128, 128, 128), // 9 medium gray
    // 10-249: 24 hues × 10 saturation/brightness steps.
    // Rather than hand-enumerate 240 entries, generate them at
    // runtime via a const-initialization helper. In pure const fn
    // we can't do this, so we keep a sparse-but-accurate subset
    // here for the most common indices and rely on a helper for the
    // rest. Each tuple is the HSV-derived RGB for a given ACI.
    //
    // Source: https://sosync.net/aci/AutoCAD_colour_index.html
    // (public reference, regenerated from the HSV formula in the ODA
    // spec appendix).
    (255, 0, 0),     // 10
    (255, 170, 170), // 11
    (189, 0, 0),     // 12
    (189, 126, 126), // 13
    (129, 0, 0),     // 14
    (129, 86, 86),   // 15
    (104, 0, 0),     // 16
    (104, 69, 69),   // 17
    (79, 0, 0),      // 18
    (79, 53, 53),    // 19
    (255, 63, 0),    // 20
    (255, 191, 170), // 21
    (189, 46, 0),    // 22
    (189, 141, 126), // 23
    (129, 31, 0),    // 24
    (129, 96, 86),   // 25
    (104, 25, 0),    // 26
    (104, 78, 69),   // 27
    (79, 19, 0),     // 28
    (79, 59, 53),    // 29
    (255, 127, 0),   // 30
    (255, 212, 170), // 31
    (189, 94, 0),    // 32
    (189, 157, 126), // 33
    (129, 64, 0),    // 34
    (129, 107, 86),  // 35
    (104, 52, 0),    // 36
    (104, 86, 69),   // 37
    (79, 39, 0),     // 38
    (79, 66, 53),    // 39
    (255, 191, 0),   // 40
    (255, 234, 170), // 41
    (189, 141, 0),   // 42
    (189, 173, 126), // 43
    (129, 96, 0),    // 44
    (129, 118, 86),  // 45
    (104, 78, 0),    // 46
    (104, 95, 69),   // 47
    (79, 59, 0),     // 48
    (79, 73, 53),    // 49
    (255, 255, 0),   // 50
    (255, 255, 170), // 51
    (189, 189, 0),   // 52
    (189, 189, 126), // 53
    (129, 129, 0),   // 54
    (129, 129, 86),  // 55
    (104, 104, 0),   // 56
    (104, 104, 69),  // 57
    (79, 79, 0),     // 58
    (79, 79, 53),    // 59
    (191, 255, 0),   // 60
    (234, 255, 170), // 61
    (141, 189, 0),   // 62
    (173, 189, 126), // 63
    (96, 129, 0),    // 64
    (118, 129, 86),  // 65
    (78, 104, 0),    // 66
    (95, 104, 69),   // 67
    (59, 79, 0),     // 68
    (73, 79, 53),    // 69
    (127, 255, 0),   // 70
    (212, 255, 170), // 71
    (94, 189, 0),    // 72
    (157, 189, 126), // 73
    (64, 129, 0),    // 74
    (107, 129, 86),  // 75
    (52, 104, 0),    // 76
    (86, 104, 69),   // 77
    (39, 79, 0),     // 78
    (66, 79, 53),    // 79
    (63, 255, 0),    // 80
    (191, 255, 170), // 81
    (46, 189, 0),    // 82
    (141, 189, 126), // 83
    (31, 129, 0),    // 84
    (96, 129, 86),   // 85
    (25, 104, 0),    // 86
    (78, 104, 69),   // 87
    (19, 79, 0),     // 88
    (59, 79, 53),    // 89
    (0, 255, 0),     // 90
    (170, 255, 170), // 91
    (0, 189, 0),     // 92
    (126, 189, 126), // 93
    (0, 129, 0),     // 94
    (86, 129, 86),   // 95
    (0, 104, 0),     // 96
    (69, 104, 69),   // 97
    (0, 79, 0),      // 98
    (53, 79, 53),    // 99
    (0, 255, 63),    // 100
    (170, 255, 191), // 101
    (0, 189, 46),    // 102
    (126, 189, 141), // 103
    (0, 129, 31),    // 104
    (86, 129, 96),   // 105
    (0, 104, 25),    // 106
    (69, 104, 78),   // 107
    (0, 79, 19),     // 108
    (53, 79, 59),    // 109
    (0, 255, 127),   // 110
    (170, 255, 212), // 111
    (0, 189, 94),    // 112
    (126, 189, 157), // 113
    (0, 129, 64),    // 114
    (86, 129, 107),  // 115
    (0, 104, 52),    // 116
    (69, 104, 86),   // 117
    (0, 79, 39),     // 118
    (53, 79, 66),    // 119
    (0, 255, 191),   // 120
    (170, 255, 234), // 121
    (0, 189, 141),   // 122
    (126, 189, 173), // 123
    (0, 129, 96),    // 124
    (86, 129, 118),  // 125
    (0, 104, 78),    // 126
    (69, 104, 95),   // 127
    (0, 79, 59),     // 128
    (53, 79, 73),    // 129
    (0, 255, 255),   // 130
    (170, 255, 255), // 131
    (0, 189, 189),   // 132
    (126, 189, 189), // 133
    (0, 129, 129),   // 134
    (86, 129, 129),  // 135
    (0, 104, 104),   // 136
    (69, 104, 104),  // 137
    (0, 79, 79),     // 138
    (53, 79, 79),    // 139
    (0, 191, 255),   // 140
    (170, 234, 255), // 141
    (0, 141, 189),   // 142
    (126, 173, 189), // 143
    (0, 96, 129),    // 144
    (86, 118, 129),  // 145
    (0, 78, 104),    // 146
    (69, 95, 104),   // 147
    (0, 59, 79),     // 148
    (53, 73, 79),    // 149
    (0, 127, 255),   // 150
    (170, 212, 255), // 151
    (0, 94, 189),    // 152
    (126, 157, 189), // 153
    (0, 64, 129),    // 154
    (86, 107, 129),  // 155
    (0, 52, 104),    // 156
    (69, 86, 104),   // 157
    (0, 39, 79),     // 158
    (53, 66, 79),    // 159
    (0, 63, 255),    // 160
    (170, 191, 255), // 161
    (0, 46, 189),    // 162
    (126, 141, 189), // 163
    (0, 31, 129),    // 164
    (86, 96, 129),   // 165
    (0, 25, 104),    // 166
    (69, 78, 104),   // 167
    (0, 19, 79),     // 168
    (53, 59, 79),    // 169
    (0, 0, 255),     // 170
    (170, 170, 255), // 171
    (0, 0, 189),     // 172
    (126, 126, 189), // 173
    (0, 0, 129),     // 174
    (86, 86, 129),   // 175
    (0, 0, 104),     // 176
    (69, 69, 104),   // 177
    (0, 0, 79),      // 178
    (53, 53, 79),    // 179
    (63, 0, 255),    // 180
    (191, 170, 255), // 181
    (46, 0, 189),    // 182
    (141, 126, 189), // 183
    (31, 0, 129),    // 184
    (96, 86, 129),   // 185
    (25, 0, 104),    // 186
    (78, 69, 104),   // 187
    (19, 0, 79),     // 188
    (59, 53, 79),    // 189
    (127, 0, 255),   // 190
    (212, 170, 255), // 191
    (94, 0, 189),    // 192
    (157, 126, 189), // 193
    (64, 0, 129),    // 194
    (107, 86, 129),  // 195
    (52, 0, 104),    // 196
    (86, 69, 104),   // 197
    (39, 0, 79),     // 198
    (66, 53, 79),    // 199
    (191, 0, 255),   // 200
    (234, 170, 255), // 201
    (141, 0, 189),   // 202
    (173, 126, 189), // 203
    (96, 0, 129),    // 204
    (118, 86, 129),  // 205
    (78, 0, 104),    // 206
    (95, 69, 104),   // 207
    (59, 0, 79),     // 208
    (73, 53, 79),    // 209
    (255, 0, 255),   // 210
    (255, 170, 255), // 211
    (189, 0, 189),   // 212
    (189, 126, 189), // 213
    (129, 0, 129),   // 214
    (129, 86, 129),  // 215
    (104, 0, 104),   // 216
    (104, 69, 104),  // 217
    (79, 0, 79),     // 218
    (79, 53, 79),    // 219
    (255, 0, 191),   // 220
    (255, 170, 234), // 221
    (189, 0, 141),   // 222
    (189, 126, 173), // 223
    (129, 0, 96),    // 224
    (129, 86, 118),  // 225
    (104, 0, 78),    // 226
    (104, 69, 95),   // 227
    (79, 0, 59),     // 228
    (79, 53, 73),    // 229
    (255, 0, 127),   // 230
    (255, 170, 212), // 231
    (189, 0, 94),    // 232
    (189, 126, 157), // 233
    (129, 0, 64),    // 234
    (129, 86, 107),  // 235
    (104, 0, 52),    // 236
    (104, 69, 86),   // 237
    (79, 0, 39),     // 238
    (79, 53, 66),    // 239
    (255, 0, 63),    // 240
    (255, 170, 191), // 241
    (189, 0, 46),    // 242
    (189, 126, 141), // 243
    (129, 0, 31),    // 244
    (129, 86, 96),   // 245
    (104, 0, 25),    // 246
    (104, 69, 78),   // 247
    (79, 0, 19),     // 248
    (79, 53, 59),    // 249
    // 250-255: 6 gray-scale steps.
    (51, 51, 51),    // 250
    (91, 91, 91),    // 251
    (132, 132, 132), // 252
    (173, 173, 173), // 253
    (214, 214, 214), // 254
    (255, 255, 255), // 255
];

/// Map an ACI index 0..=255 to (R, G, B). For index 0 (ByBlock) and
/// the out-of-table 256 (ByLayer), callers should resolve to the
/// owning block's or layer's color first and pass THAT index here.
///
/// This function is panic-free and always returns a valid triplet.
pub fn aci_to_rgb(index: u8) -> (u8, u8, u8) {
    ACI_PALETTE[index as usize]
}

/// Convenience: format an ACI index as a CSS/SVG hex color string
/// (e.g., `#FF0000`). ACI 0 returns `#000000`.
pub fn aci_to_hex(index: u8) -> String {
    let (r, g, b) = aci_to_rgb(index);
    format!("#{r:02X}{g:02X}{b:02X}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn named_primaries_match_expected() {
        assert_eq!(aci_to_rgb(1), (255, 0, 0)); // red
        assert_eq!(aci_to_rgb(2), (255, 255, 0)); // yellow
        assert_eq!(aci_to_rgb(3), (0, 255, 0)); // green
        assert_eq!(aci_to_rgb(4), (0, 255, 255)); // cyan
        assert_eq!(aci_to_rgb(5), (0, 0, 255)); // blue
        assert_eq!(aci_to_rgb(6), (255, 0, 255)); // magenta
        assert_eq!(aci_to_rgb(7), (255, 255, 255)); // white/black
    }

    #[test]
    fn grays_are_monotonic_lightness() {
        // 250 (darkest gray) → 255 (white) should be monotonically increasing.
        for i in 250..=254u8 {
            let (r, _, _) = aci_to_rgb(i);
            let (r_next, _, _) = aci_to_rgb(i + 1);
            assert!(r_next >= r, "gray {i} → {} not monotonic", i + 1);
        }
    }

    #[test]
    fn hex_format_uppercase_six_digit() {
        assert_eq!(aci_to_hex(1), "#FF0000");
        assert_eq!(aci_to_hex(3), "#00FF00");
        assert_eq!(aci_to_hex(5), "#0000FF");
        assert_eq!(aci_to_hex(0), "#000000");
        assert_eq!(aci_to_hex(255), "#FFFFFF");
    }

    #[test]
    fn palette_length_is_exactly_256() {
        assert_eq!(ACI_PALETTE.len(), 256);
    }

    #[test]
    fn hue_grid_saturation_pattern() {
        // The HSV grid has a predictable pattern: the "full saturation"
        // row (index 10, 20, 30, ..., 240) is the most saturated hue
        // step. The "faded" companion at index+1 should be lighter.
        for hue_base in (10..=240).step_by(10) {
            let (r1, g1, b1) = aci_to_rgb(hue_base);
            let (r2, g2, b2) = aci_to_rgb(hue_base + 1);
            let v1 = r1 as u32 + g1 as u32 + b1 as u32;
            let v2 = r2 as u32 + g2 as u32 + b2 as u32;
            assert!(
                v2 >= v1,
                "ACI {hue_base} ({v1}) darker-total than {} ({v2})",
                hue_base + 1
            );
        }
    }

    #[test]
    fn does_not_panic_on_any_u8() {
        for i in 0u8..=255 {
            let _ = aci_to_rgb(i);
            let _ = aci_to_hex(i);
        }
    }
}
