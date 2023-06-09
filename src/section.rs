// use crate::comment;
use crate::comment::CommentType;
use crate::commentmap::CommentMap;
use crate::{
    comment::Specialcomment,
    hashable::{ChangeState, Hashable},
};
use colored::Colorize;
use sha256::digest;

#[derive(Clone)]
pub enum Section {
    Named(SectionData, NamedSectionData),
    /// anonymous sections are sections without marker comments
    /// e.g. parts not tracked by imosid
    Anonymous(SectionData),
}

#[derive(Clone)]
pub struct NamedSectionData {
    pub name: String,           // section name, None if anonymous
    pub source: Option<String>, // source to update section from
    pub hash: String,           // current hash of section
    pub targethash: String,     // hash section should have if unmodified
}

#[derive(Clone)]
pub struct SectionData {
    pub startline: u32, // line number section starts at in file
    pub content: String,
    pub endline: u32, // line number section ends at in file
}

impl Hashable for Section {
    /// set target hash to current hash
    /// marking the section as unmodified
    /// return false if nothing has changed

    fn compile(&mut self) -> ChangeState {
        match self {
            Section::Named(_, named_data) => {
                if named_data.targethash == named_data.hash {
                    ChangeState::Unchanged
                } else {
                    named_data.targethash = named_data.hash.clone();
                    ChangeState::Changed
                }
            }
            Section::Anonymous(_) => ChangeState::Unchanged,
        }
    }

    /// generate section hash
    /// and detect section status
    fn finalize(&mut self) {
        if let Section::Named(data, named_data) = self {
            named_data.hash = digest(data.content.as_str()).to_uppercase();
        }
    }
}

impl Section {
    pub fn new(
        start: u32,
        end: u32,
        name: String,
        source: Option<String>,
        targethash: String,
    ) -> Section {
        Section::Named(
            SectionData {
                startline: start,
                content: String::from(""),
                endline: end,
            },
            NamedSectionData {
                name,
                source,
                hash: String::from(""),
                targethash,
            },
        )
    }

    pub fn from_comment_map(name: &str, map: &CommentMap) -> Option<Section> {
        Some(Section::new(
            map.get_comment(name, CommentType::SectionBegin)?.line,
            map.get_comment(name, CommentType::SectionEnd)?.line,
            name.to_string(),
            map.get_comment(name, CommentType::SourceInfo)
                .and_then(|source| source.clone().argument),
            map.get_comment(name, CommentType::HashInfo)?
                .clone()
                .argument?,
        ))
    }

    pub fn new_anonymous(start: u32, end: u32) -> Section {
        Section::Anonymous(SectionData {
            startline: start,
            content: String::from(""),
            endline: end,
        })
    }

    /// append string to content
    //maybe make this a trait?
    pub fn push_line(&mut self, line: &str) {
        match self {
            Section::Named(data, _) => data,
            Section::Anonymous(data) => data,
        }
        .content
        .push_str(format!("{}\n", line).as_str());
    }

    /// return entire section with formatted marker comments and content
    pub fn output(&self, commentsign: &str) -> String {
        match self {
            Section::Named(data, named_data) => {
                let mut outstr = String::new();
                outstr.push_str(&Specialcomment::new_string(
                    commentsign,
                    CommentType::SectionBegin,
                    &named_data.name,
                    None,
                ));
                outstr.push_str(&Specialcomment::new_string(
                    commentsign,
                    CommentType::HashInfo,
                    &named_data.name,
                    Some(&named_data.targethash),
                ));
                if let Some(source) = named_data.source.as_ref() {
                    outstr.push_str(&Specialcomment::new_string(
                        commentsign,
                        CommentType::SourceInfo,
                        &named_data.name,
                        Some(source),
                    ));
                }
                //TODO: section target
                outstr.push_str(&data.content);
                outstr.push_str(&Specialcomment::new_string(
                    commentsign,
                    CommentType::SectionEnd,
                    &named_data.name,
                    None,
                ));
                outstr
            }
            Section::Anonymous(data) => data.content.clone(),
        }
    }

    pub fn get_data(&self) -> &SectionData {
        match self {
            Section::Named(data, _) => data,
            Section::Anonymous(data) => data,
        }
    }

    pub fn pretty_info(&self) -> Option<String> {
        match self {
            Section::Anonymous(_) => None,
            Section::Named(data, named_data) => Some(format!(
                "{}-{}: {} | {}{}",
                &data.startline,
                &data.endline,
                &named_data.name,
                if named_data.targethash == named_data.hash {
                    "ok".bold().green()
                } else {
                    "modified".bold().red()
                },
                if let Some(source) = &named_data.source {
                    format!(" | source {}", source)
                } else {
                    String::new()
                }
            )),
        }
    }
}
