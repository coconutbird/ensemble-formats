//! UAX reader implementation.

use crate::{Error, Result, UAX_CHUNK_ID, UAX_FILE_ID};
use byteorder::{LittleEndian, ReadBytesExt};
use ecf::Reader;
use std::io::Cursor;

/// Parsed UAX animation data.
#[derive(Debug, Clone)]
pub struct UaxAnimation {
    /// Animation name from Granny data.
    name: Option<String>,
    /// Animation duration in seconds.
    duration: f32,
    /// Time step between keyframes.
    time_step: f32,
    /// Oversampling factor.
    oversampling: f32,
    /// Number of track groups.
    track_group_count: i32,
    /// Motion extraction mode flags from track group.
    motion_extraction_flags: u32,
}

impl UaxAnimation {
    /// Parse a UAX animation from a byte slice.
    pub fn from_bytes(data: &[u8]) -> Result<Self> {
        let ecf = Reader::new(data)?;

        // Validate file ID
        let file_id = ecf.header().id;
        if file_id != UAX_FILE_ID {
            return Err(Error::InvalidFileId(file_id));
        }

        // Find the UAX chunk (0x0700) by ID
        let chunk_index = ecf
            .chunks()
            .iter()
            .position(|c| c.id == UAX_CHUNK_ID)
            .ok_or(Error::ChunkNotFound)?;

        let chunk_data = ecf.chunk_data(chunk_index)?;

        // The chunk data has a 32-byte Granny section header before file_info.
        // Skip it to get to the actual granny_file_info structure.
        const GRANNY_HEADER_SIZE: usize = 32;
        if chunk_data.len() <= GRANNY_HEADER_SIZE {
            return Err(Error::ChunkTooSmall(
                chunk_data.len(),
                GRANNY_HEADER_SIZE + 1,
            ));
        }
        let file_info_data = &chunk_data[GRANNY_HEADER_SIZE..];

        // Parse granny_file_info from chunk
        Self::parse_granny_file_info(file_info_data)
    }

    /// Parse granny_file_info structure from chunk data.
    ///
    /// The UAX format uses a hybrid layout:
    /// - file_info structure uses 32-bit pointers (Xbox 360 origin)
    /// - Animation/TrackGroup structs use 64-bit pointers internally
    /// - Stored pointers need rebasing: actual_offset = stored_ptr - 0x10
    fn parse_granny_file_info(data: &[u8]) -> Result<Self> {
        // Minimum size to read AnimationCount and Animations pointer
        const MIN_SIZE: usize = 0x60;
        if data.len() < MIN_SIZE {
            return Err(Error::ChunkTooSmall(data.len(), MIN_SIZE));
        }

        let mut cursor = Cursor::new(data);

        // file_info uses 32-bit layout with pointer rebasing offset of 0x10
        // AnimationCount is at chunk offset 0x58 (file_info + 0x4C)
        // Animations pointer is at chunk offset 0x5C (file_info + 0x50)

        cursor.set_position(0x58);
        let animation_count = cursor.read_i32::<LittleEndian>()?;

        if animation_count < 1 {
            return Err(Error::NoAnimations);
        }

        // Read Animations pointer (32-bit, needs rebasing)
        cursor.set_position(0x5C);
        let animations_stored = cursor.read_u32::<LittleEndian>()? as u64;
        let animations_offset = rebase_pointer(animations_stored);

        if animations_offset == 0 || animations_offset as usize >= data.len() {
            return Err(Error::NoAnimations);
        }

        // TrackGroupCount is at chunk offset 0x50 (file_info + 0x44)
        // TrackGroups pointer is at chunk offset 0x54 (file_info + 0x48)
        cursor.set_position(0x50);
        let file_track_group_count = cursor.read_i32::<LittleEndian>()?;

        cursor.set_position(0x54);
        let track_groups_stored = cursor.read_u32::<LittleEndian>()? as u64;
        let track_groups_offset = rebase_pointer(track_groups_stored);

        // Get motion extraction flags from first track group if available
        let motion_extraction_flags = if file_track_group_count > 0 && track_groups_offset != 0 {
            read_track_group_flags(data, track_groups_offset)?
        } else {
            0
        };

        // Animations is animation* (direct pointer to animation struct array),
        // NOT animation** (pointer to pointer array)
        parse_animation(data, animations_offset, motion_extraction_flags)
    }

    /// Get the animation name.
    pub fn name(&self) -> Option<&str> {
        self.name.as_deref()
    }

    /// Get the animation duration in seconds.
    pub fn duration(&self) -> f32 {
        self.duration
    }

    /// Get the time step between keyframes.
    pub fn time_step(&self) -> f32 {
        self.time_step
    }

    /// Get the oversampling factor.
    pub fn oversampling(&self) -> f32 {
        self.oversampling
    }

    /// Get the number of track groups.
    pub fn track_group_count(&self) -> i32 {
        self.track_group_count
    }

    /// Get the motion extraction flags.
    pub fn motion_extraction_flags(&self) -> u32 {
        self.motion_extraction_flags
    }
}

/// Read a null-terminated C string from data at the given offset.
fn read_cstring(data: &[u8], offset: u64) -> Result<String> {
    let offset = offset as usize;
    if offset >= data.len() {
        return Err(Error::InvalidPointerOffset(offset as u64, data.len()));
    }

    let bytes = &data[offset..];
    let end = bytes
        .iter()
        .position(|&b| b == 0)
        .unwrap_or(bytes.len().min(256));

    String::from_utf8(bytes[..end].to_vec()).map_err(|_| Error::StringReadError(offset as u64))
}

/// Rebase a stored pointer to get actual offset in chunk data.
/// Granny pointers are stored with +0x10 offset.
fn rebase_pointer(stored: u64) -> u64 {
    stored.saturating_sub(0x10)
}

/// Read a u32 at the given offset.
fn read_u32_at(data: &[u8], offset: u64) -> Result<u32> {
    let offset = offset as usize;
    if offset + 4 > data.len() {
        return Err(Error::InvalidPointerOffset(offset as u64, data.len()));
    }
    let mut cursor = Cursor::new(&data[offset..]);
    Ok(cursor.read_u32::<LittleEndian>()?)
}

/// Read track group flags from the first track group.
/// track_groups_offset points to track_group* (array of 32-bit pointers to track groups)
fn read_track_group_flags(data: &[u8], track_groups_offset: u64) -> Result<u32> {
    // Read first track_group pointer (32-bit, needs rebasing)
    let tg_ptr_stored = read_u32_at(data, track_groups_offset)? as u64;
    let tg_offset = rebase_pointer(tg_ptr_stored);
    if tg_offset == 0 || tg_offset as usize >= data.len() {
        return Ok(0);
    }

    // track_group structure: Name (8), VectorTrackCount (4), VectorTracks (8), ...
    // Flags is at offset 0x50 in x64 track_group
    let flags_offset = tg_offset as usize + 0x50;
    if flags_offset + 4 > data.len() {
        return Ok(0);
    }

    let mut cursor = Cursor::new(&data[flags_offset..]);
    Ok(cursor.read_u32::<LittleEndian>()?)
}

/// Parse animation structure.
fn parse_animation(data: &[u8], offset: u64, motion_flags: u32) -> Result<UaxAnimation> {
    let offset = offset as usize;
    // animation structure (x64):
    // char const* Name;         // 0x00, 8 bytes
    // real32 Duration;          // 0x08, 4 bytes
    // real32 TimeStep;          // 0x0C, 4 bytes
    // real32 Oversampling;      // 0x10, 4 bytes
    // int32 TrackGroupCount;    // 0x14, 4 bytes
    // track_group** TrackGroups;// 0x18, 8 bytes

    if offset + 0x20 > data.len() {
        return Err(Error::InvalidPointerOffset(offset as u64, data.len()));
    }

    let mut cursor = Cursor::new(&data[offset..]);

    let name_offset = cursor.read_u64::<LittleEndian>()?;
    let duration = cursor.read_f32::<LittleEndian>()?;
    let time_step = cursor.read_f32::<LittleEndian>()?;
    let oversampling = cursor.read_f32::<LittleEndian>()?;
    let track_group_count = cursor.read_i32::<LittleEndian>()?;

    let name = if name_offset != 0 {
        Some(read_cstring(data, name_offset)?)
    } else {
        None
    };

    Ok(UaxAnimation {
        name,
        duration,
        time_step,
        oversampling,
        track_group_count,
        motion_extraction_flags: motion_flags,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_uax_files() {
        let test_files = [
            "../../temp_uax/art/campaign/npc/forge_01/shotgun_attack_01.uax",
            "../../temp_uax/art/campaign/npc/forge_01/shotgun_attack_02.uax",
            "../../temp_uax/art/campaign/npc/forge_01/shotgun_attack_03.uax",
            "../../temp_uax/art/campaign/npc/forge_01/shotgun_reload_01.uax",
        ];

        let mut parsed = 0;
        for test_path in test_files {
            if !std::path::Path::new(test_path).exists() {
                continue;
            }

            let data = std::fs::read(test_path).expect("Failed to read UAX file");
            let anim = UaxAnimation::from_bytes(&data)
                .unwrap_or_else(|e| panic!("Failed to parse {}: {:?}", test_path, e));

            // Verify parsed data
            assert!(anim.duration() > 0.0, "Duration should be positive");
            println!("Parsed {}:", test_path.rsplit('/').next().unwrap());
            println!("  Name: {:?}", anim.name());
            println!("  Duration: {:.3}s", anim.duration());
            parsed += 1;
        }

        if parsed == 0 {
            eprintln!("Skipping test - no UAX files found in temp_uax/");
        } else {
            println!("\nSuccessfully parsed {} UAX files", parsed);
        }
    }
}
