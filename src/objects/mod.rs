//! Non-entity, non-table object decoders (spec §19.5.x, §19.6.x).
//!
//! These are the structural objects that hold cross-references
//! between everything else in a DWG: the named-object DICTIONARY
//! (root + nested), XRECORD (opaque key/value storage), the
//! *_CONTROL objects (LAYER_CONTROL, BLOCK_CONTROL, STYLE_CONTROL,
//! etc.) that own the symbol-table entries, the ACAD_* named-
//! dictionary object decoders (GROUP, MLINESTYLE, PLOTSETTINGS,
//! SCALE, MATERIAL, VISUALSTYLE), plus the Phase 7 extended-data
//! decoders for XData, custom classes, proxy bodies, generic
//! dictionary walkers, and BIM-style property sets.
//!
//! | Object                   | Module                    | Spec             |
//! |--------------------------|---------------------------|------------------|
//! | ACAD_GROUP               | [`acad_group`]            | §19.6.7 (L6-11)  |
//! | ACAD_MATERIAL            | [`acad_material`]         | §19.6.9 (L6-16)  |
//! | ACAD_MLINESTYLE          | [`acad_mlinestyle`]       | §19.6.4 (L6-13)  |
//! | ACAD_PLOTSETTINGS        | [`acad_plot_settings`]    | §19.6.6 (L6-14)  |
//! | ACAD_PROPERTYSET_DATA    | [`acad_property_set_data`]| §19.6.11 (L7-07) |
//! | ACAD_SCALE               | [`acad_scale`]            | §19.6.8 (L6-15)  |
//! | ACAD_VISUALSTYLE         | [`acad_visual_style`]     | §19.6.10 (L6-17) |
//! | class-map extension      | [`class_map_extension`]   | §5.7 (L7-03)     |
//! | *_CONTROL                | [`control`]               | §19.5.1..§19.5.10|
//! | custom-dict entries      | [`custom_dict_entry`]     | §19.5.19 (L7-06) |
//! | DICTIONARY               | [`dictionary`]            | §19.5.19         |
//! | proxy entity             | [`proxy_entity`]          | §19.4.91 (L7-04) |
//! | proxy object             | [`proxy_object`]          | §19.4.91 (L7-05) |
//! | XData                    | [`xdata`]                 | §3.5 (L7-01)     |
//! | XRECORD                  | [`xrecord`]               | §19.6.5 (L7-02)  |

pub mod acad_group;
pub mod acad_material;
pub mod acad_mlinestyle;
pub mod acad_plot_settings;
pub mod acad_property_set_data;
pub mod acad_scale;
pub mod acad_visual_style;
pub mod class_map_extension;
pub mod control;
pub mod custom_dict_entry;
pub mod dictionary;
pub mod proxy_entity;
pub mod proxy_object;
pub mod xdata;
pub mod xrecord;
