use std::ptr;

use super::*;

struct Table<'a> {
    version: u16,
    encoding_records: Vec<EncodingRecord>,
    subtables: Vec<Subtable<'a>>,
}

struct EncodingRecord {
    platform_id: u16,
    encoding_id: u16,
    subtable_idx: usize,
}

struct Subtable<'a> {
    format: u16,
    language: u32,
    data: Cow<'a, [u8]>,
}

impl<'a> Structure<'a> for Table<'a> {
    fn read(r: &mut Reader<'a>) -> Result<Self> {
        let data = r.data();
        let version = r.read()?;
        let num_tables = r.read::<u16>()? as usize;
        let mut encoding_records = vec![];
        let mut subtables: Vec<Subtable<'a>> = vec![];
        for _ in 0..num_tables {
            let platform_id = r.read()?;
            let encoding_id = r.read()?;
            let offset = r.read::<u32>()? as usize;
            let format = u16::read_at(data, offset)?;
            let (length, language) = match format {
                0 | 2 | 4 | 6 => (
                    u16::read_at(data, offset + 2)? as usize,
                    u16::read_at(data, offset + 4)? as u32,
                ),
                8 | 10 | 12 | 13 => (
                    u32::read_at(data, offset + 4)? as usize,
                    u32::read_at(data, offset + 8)?,
                ),
                14 => (u32::read_at(data, offset + 2)? as usize, 0),
                _ => return Err(Error::UnknownKind),
            };
            let subtable_data = &data[offset..offset + length];
            let subtable_idx = subtables
                .iter()
                .position(|st| ptr::eq(subtable_data, st.data.as_ref()))
                .unwrap_or_else(|| {
                    let data = Cow::Borrowed(subtable_data);
                    subtables.push(Subtable { format, language, data });
                    subtables.len() - 1
                });
            encoding_records.push(EncodingRecord {
                platform_id,
                encoding_id,
                subtable_idx,
            });
        }
        Ok(Self { version, encoding_records, subtables })
    }

    fn write(&self, w: &mut Writer) {
        w.write(self.version);
        w.write(self.subtables.len() as u16);
        let mut sorted_indices = (0..self.encoding_records.len()).collect::<Vec<_>>();
        // "The encoding record entries in the 'cmap' header must be sorted
        // first by platform ID, then by platform-specific encoding ID, and then
        // by the language field in the corresponding subtable. Each platform
        // ID, platform-specific encoding ID, and subtable language combination
        // may appear only once in the 'cmap' table."
        sorted_indices.sort_by_key(|&i| {
            let rec = &self.encoding_records[i];
            (rec.platform_id, rec.encoding_id, self.subtables[rec.subtable_idx].language)
        });
        // version and n_subtables together are 4 bytes
        // each EncodingRecord is 8 bytes
        let mut offset = 4 + 8 * self.encoding_records.len() as u32;
        let mut offsets = vec![0; self.subtables.len()];
        for (i, tab) in self.subtables.iter().enumerate() {
            offsets[i] = offset;
            offset += tab.data.len() as u32;
        }
        for i in sorted_indices {
            let rec = &self.encoding_records[i];
            w.write(rec.platform_id);
            w.write(rec.encoding_id);
            w.write(offsets[rec.subtable_idx]);
        }
        for (i, tab) in self.subtables.iter().enumerate() {
            assert_eq!(offsets[i], w.len() as u32);
            w.give(tab.data.as_ref());
        }
    }
}

/// Parse a subtable with format 4, (Unicode BMP table), and convert it into an
/// equivalent table 12.
fn convert_subtable_4_to_12<'a>(st: &Subtable<'a>) -> Result<Subtable<'a>> {
    let data = st.data.as_ref();
    let seg_count_x2 = u16::read_at(data, 6)?;

    // "it is strongly recommended that parsing implementations not rely on the
    // searchRange, entrySelector and rangeShift fields in the font but derive
    // them independently from segCountX2."
    let _search_range = u16::read_at(data, 8)?;
    let _entry_selector = u16::read_at(data, 10)?;
    let _range_shift = u16::read_at(data, 12)?;

    // The greatest power of 2 less than or equal to segCountX2
    let search_range = (seg_count_x2 + 1).next_power_of_two() / 2;
    let entry_selector = search_range.trailing_zeros() as u16 - 1;
    let range_shift = seg_count_x2 - search_range;

    if cfg!(debug_assertions) {
        assert_eq!(search_range, _search_range);
        assert_eq!(entry_selector, _entry_selector);
        assert_eq!(range_shift, _range_shift);
    }

    let mut base = 14;
    let end_code = &data[base..base + seg_count_x2 as usize];
    base += seg_count_x2 as usize;
    let _reserved_pad = u16::read_at(data, base)?;
    base += 2;
    let start_code = &data[base..base + seg_count_x2 as usize];
    base += seg_count_x2 as usize;
    let id_delta = &data[base..base + seg_count_x2 as usize];
    base += seg_count_x2 as usize;
    let id_range_offset = &data[base..base + seg_count_x2 as usize];
    let _glyph_index_array = &data[base + seg_count_x2 as usize..];

    let seg_count = (seg_count_x2 / 2) as usize;

    let mut w = Writer::new();
    w.write(12u16);
    w.write(0u16); // reserved
    w.write(0u32); // length, will revisit later
    w.write(st.language);
    w.write(0u32); // nGroups, will revisit later

    let mut n_groups = 0;
    let mut write_group = |start_code: u32, end_code: u32, start_glyph_id: u32| {
        n_groups += 1;
        w.write(start_code);
        w.write(end_code);
        w.write(start_glyph_id);
    };
    for i in 0..seg_count {
        let start_code = u16::read_at(start_code, i * 2)?;
        let end_code = u16::read_at(end_code, i * 2)?;
        let id_range_offset = u16::read_at(id_range_offset, i * 2)?;
        if id_range_offset == 0 {
            let id_delta = u16::read_at(id_delta, i * 2)?;
            write_group(
                start_code as u32,
                end_code as u32,
                id_delta.wrapping_add(start_code) as u32,
            );
        } else {
            let mut pending_range: Option<(u32, u32, u32)> = None;
            for c in start_code..=end_code {
                let glyph_id = u16::read_at(
                    data,
                    base + i * 2 // &id_range_offset[i]
                        + id_range_offset as usize
                        + (c - start_code) as usize * 2,
                )?;
                // assert_eq!(glyph_id, ttf_face.glyph_index(char::from_u32(c as u32).unwrap()).unwrap().0);
                pending_range = match pending_range {
                    None => Some((c as u32, c as u32, glyph_id as u32)),
                    Some((start_code, end_code, start_glyph_id)) => {
                        if c as u32 + start_glyph_id == start_code + glyph_id as u32 {
                            Some((start_code, c as u32, start_glyph_id))
                        } else {
                            write_group(start_code, end_code, start_glyph_id);
                            Some((c as u32, c as u32, glyph_id as u32))
                        }
                    }
                }
            }
            if let Some((start_code, end_code, start_glyph_id)) = pending_range {
                write_group(start_code, end_code, start_glyph_id);
            }
        }
    }

    w.align(4);
    let mut data = w.finish();
    let length = data.len() as u32;
    data[4..8].copy_from_slice(&length.to_be_bytes());
    data[12..16].copy_from_slice(&(n_groups as u32).to_be_bytes());
    Ok(Subtable {
        format: 12,
        language: st.language,
        data: Cow::Owned(data),
    })
}

/// Maps all glyphs in the subtable to the Private Use Area (PUA) starting at
/// U+F0000 (PUA-A). The subtable must be of format 12.
fn map_glyph_to_pua_12(st: &mut Subtable<'_>, num_glyphs: u16) -> Result<()> {
    debug_assert_eq!(st.format, 12);
    let n_groups = u32::read_at(st.data.as_ref(), 12)? as usize;
    let mut groups: Vec<(u32, u32, u32)> = vec![];
    let mut cur_group = &st.data.as_ref()[16..];
    for _ in 0..n_groups {
        let start_code = u32::read_at(cur_group, 0)?;
        let end_code = u32::read_at(cur_group, 4)?;
        let start_glyph_id = u32::read_at(cur_group, 8)?;
        groups.push((start_code, end_code, start_glyph_id));
        cur_group = &cur_group[12..];
    }
    let glyph_start_code = 0xF0000;
    let glyph_end_code = glyph_start_code + num_glyphs as u32 - 1;

    // Binary search: find the first group with end_code >= glyph_start_code
    let i_start = groups.partition_point(|g| g.1 < glyph_start_code);
    // Binary search: find the first group with start_code > glyph_end_code
    let i_end = groups.partition_point(|g| g.0 <= glyph_end_code);
    if i_start == i_end {
        // Insert new group before i_start
        groups.insert(i_start, (glyph_start_code, glyph_end_code, 0));
    } else {
        // What we know about groups[i_start..i_end]:
        // - end_code >= glyph_start_code
        // - start_code <= glyph_end_code
        // This means their ranges intersect with the PUA range.
        let mut replace_with = vec![];
        {
            // groups[i_start] may have part outside the PUA range, add that part.
            let (start_code, _, start_glyph_id) = groups[i_start];
            if start_code < glyph_start_code {
                replace_with.push((start_code, glyph_start_code - 1, start_glyph_id));
            }
        }
        replace_with.push((glyph_start_code, glyph_end_code, 0));
        {
            // groups[i_end - 1] may have part outside the PUA range, add that part.
            let (start_code, end_code, start_glyph_id) = groups[i_end - 1];
            if end_code > glyph_end_code {
                replace_with.push((
                    glyph_end_code + 1,
                    end_code,
                    start_glyph_id + glyph_end_code + 1 - start_code,
                ));
            }
        }
        // Replace i_start..i_end with replace_with
        groups.splice(i_start..i_end, replace_with);
    }
    let mut w = Writer::new();
    w.give(&st.data.as_ref()[..12]);
    w.write(groups.len() as u32);
    for (start_code, end_code, start_glyph_id) in groups {
        w.write(start_code);
        w.write(end_code);
        w.write(start_glyph_id);
    }
    w.align(4);
    let mut data = w.finish();
    let length = data.len() as u32;
    data[4..8].copy_from_slice(&length.to_be_bytes());
    st.data = Cow::Owned(data);
    Ok(())
}

pub(crate) fn map_glyphs(ctx: &mut Context) -> Result<()> {
    let data = ctx.expect_table(Tag::CMAP)?;
    if !ctx.profile.map_glyphs {
        ctx.push(Tag::CMAP, data);
        return Ok(());
    }
    let mut table = Table::read(&mut Reader::new(data))?;
    let tab_12_id = match table.subtables.iter().position(|st| st.format == 12) {
        Some(id) => id,
        None => {
            let tab_4_id = table
                .subtables
                .iter()
                .position(|st| st.format == 4)
                .ok_or(Error::MissingData)?;
            table
                .subtables
                .push(convert_subtable_4_to_12(&table.subtables[tab_4_id])?);
            table.subtables.len() - 1
        }
    };

    if !table
        .encoding_records
        .iter()
        .any(|r| r.platform_id == 0 && r.encoding_id == 4)
    {
        table.encoding_records.push(EncodingRecord {
            platform_id: 0,
            encoding_id: 4,
            subtable_idx: tab_12_id,
        });
    }

    map_glyph_to_pua_12(&mut table.subtables[tab_12_id], ctx.num_glyphs)?;

    let mut writer = Writer::new();
    table.write(&mut writer);
    ctx.push(Tag::CMAP, writer.finish());
    Ok(())
}
