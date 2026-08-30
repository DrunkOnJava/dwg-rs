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
//! | LAYOUT                   | [`acad_layout`]           | §20.4.84         |
//! | ACAD_MATERIAL            | [`acad_material`]         | none — measured  |
//! | ACAD_MLINESTYLE          | [`acad_mlinestyle`]       | §19.6.4 (L6-13)  |
//! | PLOTSETTINGS             | [`acad_plot_settings`]    | §20.4.84 (block) |
//! | ACAD_PROPERTYSET_DATA    | [`acad_property_set_data`]| §19.6.11 (L7-07) |
//! | ACAD_SCALE               | [`acad_scale`]            | §19.6.8 (L6-15)  |
//! | ACAD_VISUALSTYLE         | [`acad_visual_style`]     | none — measured  |
//! | class-map extension      | [`class_map_extension`]   | §5.7 (L7-03)     |
//! | *_CONTROL                | [`control`]               | §19.5.1..§19.5.10|
//! | custom-dict entries      | [`custom_dict_entry`]     | §19.5.19 (L7-06) |
//! | ACDB_PLACEHOLDER         | [`placeholder`]           | §19.5.x          |
//! | DICTIONARY               | [`dictionary`]            | §19.5.19         |
//! | DICTIONARYVAR            | [`dictionary_var`]        | §19.6.x          |
//! | proxy entity             | [`proxy_entity`]          | §19.4.91 (L7-04) |
//! | proxy object             | [`proxy_object`]          | §19.4.91 (L7-05) |
//! | XData                    | [`xdata`]                 | §3.5 (L7-01)     |
//! | XRECORD                  | [`xrecord`]               | §19.6.5 (L7-02)  |
//!
//! # Reachability and the R2007+ split stream
//!
//! The decoders the dispatcher routes to — DICTIONARY, DICTIONARYVAR,
//! XRECORD, ACDB_PLACEHOLDER, the `*_CONTROL` family, ACAD_GROUP,
//! ACAD_SCALE, ACAD_VISUALSTYLE, LAYOUT and PLOTSETTINGS — all go
//! through the crate-private
//! `objects::modern`, so
//! their `TV` fields come
//! from the object's string stream on R2007+ and each one checks that
//! its data fields end exactly on the data-stream boundary.
//!
//! The remaining modules ([`acad_material`],
//! [`acad_property_set_data`],
//! [`acad_mlinestyle`]) decode a documented *prefix* of their record's
//! fields, or a field list this crate has not yet matched against real
//! bytes, so they cannot satisfy that boundary check and are
//! deliberately not dispatched — a partial decoder that cannot
//! self-validate would inflate the coverage ratio without proving
//! anything. They remain callable directly.
//!
//! Two of the table's "Spec" cells read *none — measured*: the ODA
//! v5.4.1 object-prescription chapter §20.4 stops at XRECORD and
//! carries no entry for VISUALSTYLE or MATERIAL. Both modules document
//! what their own byte measurements establish, and
//! [`acad_material`] restricts itself to the strings and the budget
//! precisely because its data-field layout is not among them.
//!
//! PLOTSETTINGS has no §20.4 prescription of its own either, but it
//! does not need one: §20.4.84 LAYOUT opens with the whole
//! plot-settings block, so [`acad_layout`] and [`acad_plot_settings`]
//! share a single field list and one of them is measured on 31 real
//! records.

pub mod acad_group;
pub mod acad_layout;
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
pub mod dictionary_var;
pub(crate) mod modern;
pub mod placeholder;
pub mod proxy_entity;
pub mod proxy_object;
pub mod xdata;
pub mod xrecord;
