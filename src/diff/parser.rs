use super::format::EditBlock;
use regex::Regex;

pub fn parse_diff(text: &str, file_path: &str) -> Result<Vec<EditBlock>, String> {
    let mut blocks = Vec::new();
    let re = Regex::new(
        r"(?s)<<<+ HEAD:([a-fA-F0-9]{8,})\n(.*?)\n=======\n(.*?)\n>>>+ ([a-fA-F0-9]{8,})"
    ).map_err(|e| e.to_string())?;

    for cap in re.captures_iter(text) {
        let old_anchor = cap[1].to_string();
        let old_lines = cap[2]
            .lines()
            .map(|s| s.to_string())
            .collect::<Vec<_>>();
        let new_lines = cap[3]
            .lines()
            .map(|s| s.to_string())
            .collect::<Vec<_>>();
        let new_anchor = cap[4].to_string();

        blocks.push(EditBlock {
            file_path: file_path.to_string(),
            old_anchor,
            new_anchor,
            old_lines,
            new_lines,
            context_above: 0,
            context_below: 0,
        });
    }

    if blocks.is_empty() {
        return Err("No diff blocks found in input".to_string());
    }

    Ok(blocks)
}
