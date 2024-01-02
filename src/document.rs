use crate::util;
use std::{
    io::{BufRead, BufReader, Read},
    path::{Path, PathBuf},
};

extern crate serde;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Link {
    #[serde(rename = "type")]
    pub link_type: String,
    pub description: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct DocLink {
    pub url: String,
    pub title: String,
    pub summary: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, Clone, Default)]
pub struct DocumentMeta {
    #[serde(default)]
    pub position: u32,
    #[serde(rename = "label")]
    pub title: String,
    #[serde(default)]
    pub date: String,
    #[serde(default)]
    pub slug: String,
    #[serde(default)]
    pub author: String,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub link: Option<Link>,
}

#[derive(Debug, Clone)]
pub struct Document {
    pub path: PathBuf,
    pub html: String,
    pub meta: DocumentMeta,
}

// Used when rendering templates.
#[derive(Serialize, Deserialize)]
pub struct PageContext {
    pub title: Option<String>,
    pub contents: Option<String>,
    pub sidebar: Option<String>,
    pub children: Vec<DocLink>,
    pub prev: Option<DocLink>,
    pub next: Option<DocLink>,
    pub year: i32,
}

impl PageContext {
    pub fn new(title: Option<String>, contents: Option<String>) -> Self {
        Self {
            title,
            contents,
            sidebar: None,
            year: 2024,
            prev: None,
            next: None,
            children: vec![],
        }
    }
}

impl Document {
    pub fn to_doclink(&self) -> DocLink {
        DocLink {
            title: self.meta.title.clone(),
            url: format!("/{}", self.path.display()),
            summary: None, // TODO
        }
    }

    fn read_dash_meta(reader: &mut impl BufRead) -> anyhow::Result<(DocumentMeta, bool)> {
        let mut meta = DocumentMeta::default();
        let mut buffer = String::new();

        reader.read_line(&mut buffer)?;

        if !buffer.starts_with("---") {
            // Won't be anything to read.
            return if let Some(suffix) = buffer.strip_prefix("# ") {
                meta.title = suffix.trim().to_string();
                Ok((meta, true))
            } else {
                Ok((meta, false))
            };
        }

        let mut found_end = false;
        buffer.clear();
        while let Ok(_) = reader.read_line(&mut buffer) {
            if buffer.starts_with("---") {
                found_end = true;
                break;
            }
            if let Some((key, value)) = buffer.split_once(':') {
                let value = value.trim().to_owned();
                match key {
                    "title" => meta.title = value,
                    "slug" => meta.slug = value,
                    "authors" => meta.author = value,
                    "tags" => meta.tags = value.split(' ').map(str::to_owned).collect::<Vec<_>>(),
                    _ => {}
                }
            }

            buffer.clear();
        }

        // TODO: runtime error
        assert!(found_end);
        Ok((meta, false))
    }

    // Handles page, blog posts, etc, including triple-dash docusaurus-style metadata.
    pub fn from_md(md_path: &Path, options: &markdown::Options) -> anyhow::Result<Self> {
        let md_file = std::fs::File::open(&md_path)?;
        let mut reader = BufReader::new(md_file);
        let (mut meta, mut ate_title) = Self::read_dash_meta(&mut reader)?;

        let mut md = String::from("");
        // If no dash-meta, grab the title string.
        if meta.title.is_empty() {
            let mut buffer = String::new();
            let _ = reader.read_line(&mut buffer);
            if let Some(remaining) = buffer.strip_prefix("# ") {
                meta.title = remaining.to_string(); // TODO: Only open the file once. But who cares, really.
                ate_title = true;
            }
        }
        if ate_title {
            md += &format!("# {}\n", meta.title);
        }

        // OK, read the rest.
        let mut buffer = vec![];
        reader.read_to_end(&mut buffer)?;

        md += &String::from_utf8(buffer)?;
        let html = markdown::to_html_with_options(&md, options).map_err(anyhow::Error::msg)?;
        let mut path = md_path.to_path_buf();
        path.set_extension("");
        Ok(Self { path, html, meta })
    }

    // The document itself is the template so we apply it immediately. Used for pages.
    pub fn from_hbs(
        hbs_path: &Path,
        handlebars: &mut handlebars::Handlebars,
    ) -> anyhow::Result<Self> {
        let hbs = std::fs::read_to_string(&hbs_path)?;
        let context = PageContext::new(None, None);
        let html = handlebars.render_template(&hbs, &context)?;

        Ok(Self {
            path: hbs_path.to_path_buf(),
            meta: DocumentMeta::default(),
            html,
        })
    }

    // Applies the "doc" template.
    pub fn from_html(html_path: &Path) -> anyhow::Result<Self> {
        let html = std::fs::read_to_string(&html_path)?;
        Ok(Self {
            path: html_path.to_path_buf(),
            html,
            meta: DocumentMeta {
                title: "untitled html".to_string(),
                ..Default::default()
            },
        })
    }

    // Category front pages.
    pub fn from_category(
        category: &Category,
        handlebars: &mut handlebars::Handlebars,
    ) -> anyhow::Result<Self> {
        let mut context = PageContext::new(None, None);

        context.children = category
            .documents
            .iter()
            .map(|doc| doc.to_doclink())
            .collect::<Vec<_>>();

        // Add children document titles to the context here.

        let html = handlebars.render("cat_contents", &context)?;
        Ok(Self {
            path: category.path.clone(),
            html,
            meta: category.meta.clone(),
        })
    }
}

#[derive(Debug, Clone)]
pub struct Category {
    pub meta: DocumentMeta,
    pub documents: Vec<Document>,
    pub sub_categories: Vec<Category>,
    pub path: PathBuf,
}

impl Category {
    pub fn from_folder_tree(folder: &Path, options: &markdown::Options) -> anyhow::Result<Self> {
        let mut documents = vec![];
        let mut sub_categories = vec![];
        let listing = folder.read_dir()?;
        let mut meta = DocumentMeta::default();
        meta.title = "Documentation".to_string();

        for dir_entry in listing {
            let entry = dir_entry?;
            let path = folder.join(entry.file_name());
            let name = util::filename_to_string(&entry.file_name());
            if entry.metadata()?.is_dir() {
                sub_categories.push(Self::from_folder_tree(&path, options)?);
            } else if let Some(os_str) = path.extension() {
                // Check file extension to figure out what to do.
                match os_str.to_str().unwrap() {
                    "md" => {
                        documents.push(Document::from_md(&path, options)?);
                    }
                    "json" => {
                        if name == "_category_.json" {
                            let json_str = std::fs::read_to_string(path)?;
                            meta = serde_json::from_str(&json_str).expect("Failed to parse JSON");
                        }
                    }
                    _ => {
                        anyhow::bail!("Unhandled file type: {}", path.display());
                    }
                }
            }
        }

        Ok(Self {
            meta,
            documents,
            sub_categories,
            path: folder.to_path_buf(),
        })
    }

    // Note: This also generates category documents.

    pub fn all_documents(
        &self,
        handlebars: &mut handlebars::Handlebars,
    ) -> anyhow::Result<Vec<Document>> {
        let mut all_docs = vec![];
        all_docs.push(Document::from_category(self, handlebars)?);
        for doc in &self.documents {
            all_docs.push(doc.clone());
        }
        for cat in &self.sub_categories {
            all_docs.extend(cat.all_documents(handlebars)?);
        }
        Ok(all_docs)
    }

    pub fn all_categories(&self) -> Vec<Category> {
        let mut all_cats = vec![];
        for cat in &self.sub_categories {
            all_cats.extend(cat.all_categories());
        }
        all_cats
    }
}
