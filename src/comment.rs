use regex::Regex;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
// give targetinfo sourceinfo, hashinfo and targetinfo required parameter fields
pub enum CommentType {
    SectionBegin,
    SectionEnd,
    SourceInfo,
    TargetInfo,
    HashInfo,
    PermissionInfo,
}

impl CommentType {
    pub fn from_keyword(keyword: &str) -> Option<CommentType> {
        Some(match keyword {
            "begin" | "start" => CommentType::SectionBegin,
            "end" | "stop" => CommentType::SectionEnd,
            "hash" => CommentType::HashInfo,
            "source" => CommentType::SourceInfo,
            "permissions" => CommentType::PermissionInfo,
            "target" => CommentType::TargetInfo,
            &_ => {
                return Option::None;
            }
        })
    }
}

impl Into<String> for CommentType {
    fn into(self) -> String {
        String::from(match self {
            CommentType::SectionBegin => "begin",
            CommentType::SectionEnd => "end",
            CommentType::SourceInfo => "source",
            CommentType::TargetInfo => "target",
            CommentType::HashInfo => "hash",
            CommentType::PermissionInfo => "permissions",
        })
    }
}

#[derive(Clone)]
pub struct Specialcomment {
    pub line: u32,       // line number comment is at in file
    pub section: String, // section name extracted from prefix
    pub comment_type: CommentType,
    pub argument: Option<String>, // optional argument, used for hashes etc
}

impl Specialcomment {
    pub fn new_string(
        commentsymbol: &str,
        ctype: CommentType,
        section_name: &str,
        argument: Option<&str>,
    ) -> String {
        format!(
            "{}... {} {}{}\n",
            commentsymbol,
            section_name,
            Into::<String>::into(ctype),
            if argument.is_some() {
                format!(" {}", argument.unwrap())
            } else {
                String::from("")
            }
        )
    }

    pub fn from_line(line: &str, commentsymbol: &str, linenumber: u32) -> Option<Specialcomment> {
        if !line.starts_with(commentsymbol) {
            return Option::None;
        }

        // construct regex that matches valid comments
        let mut iscomment = String::from("^ *");
        iscomment.push_str(&commentsymbol);
        iscomment.push_str(" *\\.\\.\\. *(.*)");
        let commentregex = Regex::new(&iscomment).unwrap();

        let keywords = commentregex.captures(&line);

        if let Some(captures) = &keywords {
            let keywords = captures
                .get(1)
                .unwrap()
                .as_str()
                .split(" ")
                .collect::<Vec<&str>>();

            // needs at least a section and a keyword
            if keywords.len() < 2 {
                return Option::None;
            }

            let sectionname = keywords[0];
            let keyword = keywords[1];
            //comment argument, example #...all source ARGUMENT
            let cargument: Option<String> = if keywords.len() > 2 {
                Option::Some(String::from(keywords[2]))
            } else {
                Option::None
            };

            let tmptype: CommentType;
            tmptype = CommentType::from_keyword(keyword)?;
            match tmptype {
                CommentType::HashInfo => {
                    if cargument == None {
                        println!("missing hash value on line {}", linenumber);
                        return Option::None;
                    }
                }
                CommentType::SourceInfo => {
                    if cargument.is_some() {
                        println!("updating from source not implemented yet");
                        unimplemented!();
                        //TODO do something
                        //fetch from file/url/git
                    } else {
                        println!("missing source file argument on line {}", linenumber);
                        return Option::None;
                    }
                }
                CommentType::PermissionInfo => {
                    // permissioms can only be set for the entire file
                    if sectionname != "all" {
                        return Option::None;
                    }
                    match &cargument {
                        None => {
                            return Option::None;
                        }
                        //todo: more validation. maybe own permission type?
                        Some(arg) => match arg.parse::<u32>() {
                            Err(_) => {
                                return Option::None;
                            }
                            Ok(_) => {}
                        },
                    }
                }
                CommentType::TargetInfo => {
                    if sectionname == "all" {
                        if cargument == None {
                            println!("missing target value on line {}", linenumber);
                            return Option::None;
                        }
                    } else {
                        println!(
                            "warning: target can only apply to the whole file {}",
                            linenumber
                        );
                        return Option::None;
                    }
                }
                _ => {
                    println!("warning: incomplete imosid comment on {}", linenumber);
                    return Option::None;
                }
            }

            return Some(Specialcomment {
                line: linenumber,
                section: String::from(sectionname),
                comment_type: tmptype,
                argument: cargument,
            });
        };
        return Option::None;
    }
}
