use reqwest::header::{HeaderMap, HeaderValue, USER_AGENT};
use select::{document::Document, node::Node};
use serde_json::Value;
use std::{collections::HashMap, io::Write};
use toml::Value as TValue;
use unicode_categories::UnicodeCategories;
use unicode_segmentation::UnicodeSegmentation;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let categorized_emoji_ids = get_categorized_github_emoji_ids().await?;

    let toml_str = std::fs::read_to_string("Cargo.toml").expect("Unable to read file");
    let toml_val = toml_str.parse::<TValue>().expect("Unable to parse TOML");
    let repo_name = toml_val["package"]["name"].as_str().unwrap();

    let resource1 = "GitHub Emoji API";
    let resource2 = "Unicode Full Emoji List";
    let columns = 2;
    let toc_name = "Table of Contents";

    let cheat_sheet = generate_cheat_sheet(
        &repo_name,
        &resource1,
        &resource2,
        columns,
        &toc_name,
        &categorized_emoji_ids,
    );

    let mut file = std::fs::File::create("readme.md").expect("Unable to create file");
    file.write_all(cheat_sheet.as_bytes())
        .expect("Unable to write data to file");
    Ok(())
}

async fn get_categorized_github_emoji_ids(
) -> Result<HashMap<String, HashMap<String, Vec<Vec<String>>>>, Box<dyn std::error::Error>> {
    let github_emoji_id_map = get_github_emoji_id_map().await?;
    let categorized_emoji_ids = categorize_github_emoji_ids(github_emoji_id_map).await?;
    Ok(categorized_emoji_ids)
}

async fn get_github_emoji_id_map(
) -> Result<HashMap<String, EmojiLiteral>, Box<dyn std::error::Error>> {
    let url = "https://api.github.com/emojis";
    let mut headers = HeaderMap::new();
    headers.insert(
        USER_AGENT,
        HeaderValue::from_str("https://github.com/ikatyang/emoji-cheat-sheet")?,
    );

    let response = reqwest::Client::new()
        .get(url)
        .headers(headers)
        .send()
        .await?;
    let json: Value = serde_json::from_slice(&response.bytes().await?)?;

    let mut github_emoji_id_map = HashMap::new();
    for (id, url) in json.as_object().unwrap().iter() {
        let emoji_literal = if url.as_str().unwrap().contains("/unicode/") {
            let code_points: Vec<_> = url
                .as_str()
                .unwrap()
                .split('/')
                .last()
                .unwrap()
                .split(".png")
                .next()
                .unwrap()
                .split('-')
                .map(|code_point_text| {
                    std::char::from_u32(u32::from_str_radix(code_point_text, 16).unwrap()).unwrap()
                })
                .collect::<Vec<_>>();
            EmojiLiteral::Unicode(code_points)
        } else {
            let custom_emoji = vec![url
                .as_str()
                .unwrap()
                .split('/')
                .last()
                .unwrap()
                .split(".png")
                .next()
                .unwrap()
                .to_string()];
            EmojiLiteral::Custom(custom_emoji)
        };
        github_emoji_id_map.insert(id.to_string(), emoji_literal);
    }
    // println!("{:?}", github_emoji_id_map);
    Ok(github_emoji_id_map)
}

async fn categorize_github_emoji_ids(
    github_emoji_id_map: HashMap<String, EmojiLiteral>,
) -> Result<HashMap<String, HashMap<String, Vec<Vec<String>>>>, Box<dyn std::error::Error>> {
    let url = "https://unicode.org/emoji/charts/full-emoji-list.html";
    let html_text = reqwest::get(url).await?.text().await?;
    let doc = Document::from(html_text.as_str());

    let mut github_specific_emoji_uri_to_github_emoji_ids_map = HashMap::new();
    let mut emoji_literal_to_github_emoji_ids_map = HashMap::new();
    for (emoji_id, emoji_literal) in github_emoji_id_map {
        match emoji_literal {
            EmojiLiteral::Unicode(emoji_code_points) => {
                let emoji_literal_str: String = emoji_code_points.into_iter().collect();
                emoji_literal_to_github_emoji_ids_map
                    .entry(emoji_literal_str)
                    .or_insert_with(Vec::new)
                    .push(emoji_id);
            }
            EmojiLiteral::Custom(uri) => {
                github_specific_emoji_uri_to_github_emoji_ids_map
                    .entry(uri[0].clone())
                    .or_insert_with(Vec::new)
                    .push(emoji_id);
            }
        }
    }

    let mut categorized_emoji_ids: HashMap<String, HashMap<String, Vec<Vec<String>>>> =
        HashMap::new();
    let mut category_stack: Vec<String> = Vec::new();

    for tr in doc.find(tr_predicate) {
        let child = tr.first_child();
        if let Some(th) = child {
            if th.name() == Some("th") {
                if let Some(class) = th.attr("class") {
                    if class == "bighead" {
                        category_stack.clear();
                        let title = to_title_case(th.text());
                        category_stack.push(title.clone());
                        categorized_emoji_ids.insert(title, HashMap::new());
                    } else if class == "mediumhead" {
                        if category_stack.len() > 1 {
                            category_stack.pop();
                        }
                        let title = to_title_case(th.text());
                        category_stack.push(title.clone());
                        categorized_emoji_ids
                            .entry(category_stack[0].clone())
                            .or_insert_with(HashMap::new)
                            .insert(title, Vec::new());
                    }
                }
            } else {
                if let Some(emoji) = tr.find(td_chars_predicate).next() {
                    let key: String = emoji
                        .text()
                        .graphemes(true)
                        .filter(|g| g.chars().any(|c| !c.is_mark_spacing_combining()))
                        .flat_map(|g| g.chars())
                        .collect();
                    if let Some(github_emoji_ids) = emoji_literal_to_github_emoji_ids_map.get(&key)
                    {
                        let category = &category_stack[0];
                        let subcategory = &category_stack[1];
                        let github_emoji_ids = github_emoji_ids.clone();
                        categorized_emoji_ids
                            .entry(category.clone())
                            .or_insert_with(HashMap::new)
                            .entry(subcategory.clone())
                            .or_insert_with(Vec::new)
                            .push(github_emoji_ids);
                    }
                }
            }
        }
    }

    if !github_specific_emoji_uri_to_github_emoji_ids_map.is_empty() {
        let custom_emojis: Vec<Vec<String>> = github_specific_emoji_uri_to_github_emoji_ids_map
            .values()
            .map(|v| v.clone())
            .collect();
        categorized_emoji_ids.insert(
            "GitHub Custom Emoji".to_string(),
            [("".to_string(), custom_emojis)].iter().cloned().collect(),
        );
    }

    Ok(categorized_emoji_ids)
}

fn tr_predicate(node: &Node) -> bool {
    node.name() == Some("tr")
}

fn td_chars_predicate(node: &Node) -> bool {
    // if node.name() != Some("td") {
    //     return false;
    // }
    if let Some(class) = node.attr("class") {
        class == "chars"
    } else {
        false
    }
}

fn to_title_case(s: String) -> String {
    s.replace("-", " ")
        .split_whitespace()
        .map(|word| {
            let mut c = word.chars();
            match c.next() {
                None => String::new(),
                Some(f) => f.to_uppercase().chain(c).collect(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

#[derive(Debug)]
enum EmojiLiteral {
    Unicode(Vec<char>),
    Custom(Vec<String>),
}

fn generate_cheat_sheet(
    repo_name: &str,
    resource1: &str,
    resource2: &str,
    columns: usize,
    toc_name: &str,
    categorized_github_emoji_ids: &HashMap<String, HashMap<String, Vec<Vec<String>>>>,
) -> String {
    let mut line_texts = Vec::new();

    line_texts.push(format!("# {}", repo_name));
    line_texts.push("".to_string());
    line_texts.push("".to_string());
    line_texts.push(format!(
        "This cheat sheet is automatically generated from [{}]({}) and [{}]({}).",
        resource1,
        "https://api.github.com/emojis",
        resource2,
        "https://unicode.org/emoji/charts/full-emoji-list.html"
    ));
    line_texts.push("".to_string());

    let categories: Vec<&String> = categorized_github_emoji_ids.keys().collect();

    line_texts.push(format!("## {}", toc_name));
    line_texts.push("".to_string());
    line_texts.extend(generate_toc(&categories));
    line_texts.push("".to_string());

    for category in &categories {
        line_texts.push(format!("### {}", category));
        line_texts.push("".to_string());

        let subcategorize_github_emoji_ids = &categorized_github_emoji_ids[*category];
        let subcategories: Vec<&String> = subcategorize_github_emoji_ids.keys().collect();
        if subcategories.len() > 1 {
            line_texts.extend(generate_toc(&subcategories));
            line_texts.push("".to_string());
        }

        for subcategory in &subcategories {
            if !subcategory.is_empty() {
                line_texts.push(format!("#### {}", subcategory));
                line_texts.push("".to_string());
            }

            line_texts.extend(generate_table(
                &subcategorize_github_emoji_ids[*subcategory],
                columns,
                &format!("[top](#{})", get_header_id(category)),
                &format!("[top](#{})", get_header_id(toc_name)),
            ));
            line_texts.push("".to_string());
        }
    }

    line_texts.join("\n")
}

fn generate_toc(headers: &[&String]) -> Vec<String> {
    headers
        .iter()
        .map(|header| format!("- [{}](#{})", header, get_header_id(header)))
        .collect()
}

fn get_header_id(header: &str) -> String {
    header
        .to_lowercase()
        .replace(" ", "-")
        .replace(|c: char| !c.is_ascii_alphanumeric() && c != '-', "")
}

fn generate_table(
    github_emoji_ids: &Vec<Vec<String>>,
    columns: usize,
    left_text: &str,
    right_text: &str,
) -> Vec<String> {
    println!("{:?}", github_emoji_ids);
    let mut line_texts = Vec::new();

    let mut header = "| ".to_string();
    let mut delimiter = "| - ".to_string();
    for _ in 0..columns.min(github_emoji_ids.len()) {
        header += "| ico | shortcode ";
        delimiter += "| :-: | - ";
    }
    header += "| |";
    delimiter += "| - |";

    line_texts.push(header);
    line_texts.push(delimiter);

    for i in (0..github_emoji_ids.len()).step_by(columns) {
        let mut line_text = format!("| {} ", left_text);
        for j in 0..columns {
            if i + j < github_emoji_ids.len() {
                let emoji_ids = &github_emoji_ids[i + j];
                let emoji_id = &emoji_ids[0];
                line_text += &format!("| :{}: | `:{}:` ", emoji_id, emoji_id);
                for k in 1..emoji_ids.len() {
                    line_text += &format!("<br /> `:{}:` ", emoji_ids[k]);
                }
            } else if github_emoji_ids.len() > columns {
                line_text += "| | ";
            }
        }
        line_text += &format!("| {} |", right_text);
        line_texts.push(line_text);
    }

    line_texts
}
