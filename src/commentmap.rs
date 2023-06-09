use std::collections::{HashMap, HashSet};

use crate::comment::{CommentType, Specialcomment};

pub struct CommentMap {
    map: HashMap<String, Vec<Specialcomment>>,
    potentially_invalid: bool,
}

impl CommentMap {
    pub fn new() -> Self {
        Self {
            map: HashMap::new(),
            potentially_invalid: false,
        }
    }

    pub fn push_comment(&mut self, comment: Specialcomment) {
        if let Some(vec) = self.map.get_mut(&comment.section) {
            vec.push(comment);
        } else {
            self.map.insert(comment.section.clone(), vec![comment]);
        }
        self.potentially_invalid = true;
    }

    pub fn remove_incomplete(&mut self) {
        let mut incomplete_sections = vec![];
        for (section, comments) in self.map.iter() {
            if section == "all" {
                continue;
            }
            let mut incomplete = false;
            let mut comment_types: HashSet<CommentType> = HashSet::new();
            for comment in comments {
                // do not allow for multiple definitions of the same attribute
                if comment_types.contains(&comment.comment_type) {
                    incomplete = true;
                    break;
                }
                comment_types.insert(comment.comment_type.clone());
            }

            if !incomplete {
                incomplete = !comment_types.contains(&CommentType::SectionBegin)
                    || !comment_types.contains(&CommentType::HashInfo)
                    || !comment_types.contains(&CommentType::SectionEnd);
            }
            if incomplete {
                incomplete_sections.push(section.clone());
            }
        }

        for section in incomplete_sections {
            self.remove_section(&section);
        }
        self.potentially_invalid = false;
    }

    pub fn remove_section(&mut self, section: &str) {
        self.map.remove(section);
    }

    pub fn get_comments(&self, section: &str) -> Option<&Vec<Specialcomment>> {
        self.map.get(section)
    }

    pub fn get_sections(&self) -> Vec<&String> {
        self.map
            .keys()
            .filter(|section| section.as_str() != "all")
            .collect()
    }

    pub fn get_comment(&self, section: &str, comment_type: CommentType) -> Option<&Specialcomment> {
        if let Some(comments) = self.map.get(section) {
            for comment in comments {
                if comment.comment_type == comment_type {
                    return Some(comment);
                }
            }
        }
        None
    }
}
