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
//! | ACDBDETAILVIEWSTYLE      | [`acad_detail_view_style`]| none — measured  |
//! | ACAD_GROUP               | [`acad_group`]            | §19.6.7 (L6-11)  |
//! | LAYOUT                   | [`acad_layout`]           | §20.4.84         |
//! | ACAD_MATERIAL            | [`acad_material`]         | none — measured  |
//! | ACDB(ASSOC)PERSSUBENTMANAGER | [`pers_subent`]       | none — measured  |
//! | MLEADERSTYLE             | [`acad_mleader_style`]    | none — measured  |
//! | MLINESTYLE               | [`acad_mlinestyle`]       | §20.4.73         |
//! | PLOTSETTINGS             | [`acad_plot_settings`]    | §20.4.84 (block) |
//! | ACAD_PROPERTYSET_DATA    | [`acad_property_set_data`]| §19.6.11 (L7-07) |
//! | ACAD_SCALE               | [`acad_scale`]            | §19.6.8 (L6-15)  |
//! | TABLESTYLE (until R2007) | [`acad_table_style`]      | §20.4.101.1      |
//! | ACDBSECTIONVIEWSTYLE     | [`acad_section_view_style`]| none — measured |
//! | ACAD_VISUALSTYLE         | [`acad_visual_style`]     | none — measured, every release |
//! | class-map extension      | [`class_map_extension`]   | §5.7 (L7-03)     |
//! | *_CONTROL                | [`control`]               | §19.5.1..§19.5.10|
//! | custom-dict entries      | [`custom_dict_entry`]     | §19.5.19 (L7-06) |
//! | ACDB_PLACEHOLDER         | [`placeholder`]           | §19.5.x          |
//! | RASTERVARIABLES          | [`image_raster_variables`]      | §20.4.91         |
//! | DICTIONARY               | [`dictionary`]            | §19.5.19         |
//! | DICTIONARYVAR            | [`dictionary_var`]        | §19.6.x          |
//! | IMAGEDEF_REACTOR         | [`image_def_reactor`]     | §20.4.82         |
//! | proxy entity             | [`proxy_entity`]          | §19.4.91 (L7-04) |
//! | proxy object             | [`proxy_object`]          | §19.4.91 (L7-05) |
//! | XData                    | [`xdata`]                 | §3.5 (L7-01)     |
//! | SPATIAL_FILTER           | [`clip_spatial_filter`]        | §20.4.94         |
//! | XRECORD                  | [`xrecord`]               | §19.6.5 (L7-02)  |
//! | SORTENTSTABLE            | [`block_sortents_table`]        | §20.4.93         |
//!
//! # Reachability and the R2007+ split stream
//!
//! The decoders the dispatcher routes to — DICTIONARY, DICTIONARYVAR,
//! XRECORD, ACDB_PLACEHOLDER, the `*_CONTROL` family, ACAD_GROUP,
//! ACAD_SCALE, ACAD_VISUALSTYLE, LAYOUT, PLOTSETTINGS, MLINESTYLE,
//! MLEADERSTYLE, ACDBDETAILVIEWSTYLE and ACDBSECTIONVIEWSTYLE — all go
//! through the crate-private
//! `objects::modern`, so
//! their `TV` fields come
//! from the object's string stream on R2007+ and each one checks that
//! its data fields end exactly on the data-stream boundary.
//!
//! The remaining modules ([`acad_material`] and
//! [`acad_property_set_data`]) decode a documented *prefix* of their
//! record's fields, so they cannot satisfy that boundary check and are
//! deliberately not dispatched — a partial decoder that cannot
//! self-validate would inflate the coverage ratio without proving
//! anything. They remain callable directly.
//!
//! Five of the table's "Spec" cells read *none — measured*: the ODA
//! v5.4.1 object-prescription chapter §20.4 carries no entry for
//! VISUALSTYLE, MATERIAL, MLEADERSTYLE, ACDBDETAILVIEWSTYLE or
//! ACDBSECTIONVIEWSTYLE. Each module documents
//! what its own byte measurements establish, and
//! [`acad_material`] restricts itself to the strings and the budget
//! precisely because its data-field layout is not among them.
//!
//! TABLESTYLE is decoded from §20.4.101.1 on R14 to R2007 ([`acad_table_style`]);
//! its R2010+ record is §20.4.101.2's different layout and stays
//! `Unhandled`.
//!
//! PLOTSETTINGS has no §20.4 prescription of its own either, but it
//! does not need one: §20.4.84 LAYOUT opens with the whole
//! plot-settings block, so [`acad_layout`] and [`acad_plot_settings`]
//! share a single field list and one of them is measured on 31 real
//! records.

pub mod acad_detail_view_style;
pub mod acad_group;
pub mod acad_layout;
pub mod acad_material;
pub mod acad_mleader_style;
pub mod acad_mlinestyle;
pub mod acad_plot_settings;
pub mod acad_property_set_data;
pub mod acad_scale;
pub mod acad_section_view_style;
pub mod acad_table_style;
pub mod acad_visual_style;
pub mod block_sortents_table;
pub mod class_map_extension;
pub mod clip_spatial_filter;
pub mod color;
pub mod control;
pub mod custom_dict_entry;
pub mod dictionary;
pub mod dictionary_var;
pub mod image_def_reactor;
pub mod image_raster_variables;
pub(crate) mod modern;
pub mod pers_subent;
pub mod placeholder;
pub mod proxy_entity;
pub mod proxy_object;
pub mod xdata;
pub mod xrecord;
