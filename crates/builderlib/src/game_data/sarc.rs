// Fast standalone SARC archive reader and unpacker in Rust.

use std::collections::HashMap;

#[allow(dead_code)]
#[derive(Debug)]
pub struct SarcFile {
    pub name: String,
    pub data: Vec<u8>,
}

pub fn read_sarc(data: &[u8]) -> Result<HashMap<String, Vec<u8>>, &'static str> {
    if data.len() < 0x20 || &data[0..4] != b"SARC" {
        return Err("Not a valid SARC archive");
    }

    let bom = &data[6..8];
    let little_endian = bom == b"\xFF\xFE";
    if !little_endian {
        return Err("Big-endian SARC not supported");
    }

    let header_len = u16::from_le_bytes(data[4..6].try_into().unwrap()) as usize;
    let data_offset = u32::from_le_bytes(data[12..16].try_into().unwrap()) as usize;

    let sfat_offset = header_len;
    if sfat_offset + 0x10 > data.len() || &data[sfat_offset..sfat_offset + 4] != b"SFAT" {
        return Err("SFAT block not found");
    }

    let sfat_header_len = u16::from_le_bytes(data[sfat_offset + 4..sfat_offset + 6].try_into().unwrap()) as usize;
    let node_count = u16::from_le_bytes(data[sfat_offset + 6..sfat_offset + 8].try_into().unwrap()) as usize;

    let nodes_start = sfat_offset + sfat_header_len;
    let sfnt_offset = nodes_start + (node_count * 16);

    let has_names = sfnt_offset + 8 <= data.len() && &data[sfnt_offset..sfnt_offset + 4] == b"SFNT";
    let name_table_offset = if has_names {
        let sfnt_header_len = u16::from_le_bytes(data[sfnt_offset + 4..sfnt_offset + 6].try_into().unwrap()) as usize;
        sfnt_offset + sfnt_header_len
    } else {
        0
    };

    let mut map = HashMap::with_capacity(node_count);

    for i in 0..node_count {
        let node_off = nodes_start + i * 16;
        if node_off + 16 > data.len() {
            break;
        }

        let name_hash = u32::from_le_bytes(data[node_off..node_off + 4].try_into().unwrap());
        let name_flag_off = u32::from_le_bytes(data[node_off + 4..node_off + 8].try_into().unwrap());
        let node_start = u32::from_le_bytes(data[node_off + 8..node_off + 12].try_into().unwrap()) as usize;
        let node_end = u32::from_le_bytes(data[node_off + 12..node_off + 16].try_into().unwrap()) as usize;

        let file_start = data_offset + node_start;
        let file_end = data_offset + node_end;
        if file_start > data.len() || file_end > data.len() || file_start > file_end {
            continue;
        }
        let file_bytes = data[file_start..file_end].to_vec();

        let filename = if has_names && (name_flag_off & 0x01000000) != 0 {
            let str_off = ((name_flag_off & 0x00FFFFFF) * 4) as usize;
            let str_start = name_table_offset + str_off;
            if str_start < data.len() {
                let null_pos = data[str_start..].iter().position(|&b| b == 0).unwrap_or(data.len() - str_start);
                String::from_utf8_lossy(&data[str_start..str_start + null_pos]).into_owned()
            } else {
                format!("file_{:04}_{:08X}.bin", i, name_hash)
            }
        } else {
            format!("file_{:04}_{:08X}.bin", i, name_hash)
        };

        map.insert(filename, file_bytes);
    }

    Ok(map)
}
