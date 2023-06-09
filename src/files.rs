use crate::comment::{CommentType, Specialcomment};
use crate::commentmap::CommentMap;
use crate::contentline::ContentLine;
use crate::hashable::Hashable;
use crate::metafile::MetaFile;
use crate::section::{NamedSectionData, Section, SectionData};
use colored::Colorize;
use regex::Regex;
use std::clone;
use std::collections::HashMap;
use std::ffi::OsStr;
use std::fs::{self, File, OpenOptions};

use std::io::prelude::*;
use std::io::{self, ErrorKind};
use std::ops::Deref;
use std::os::unix::prelude::PermissionsExt;
use std::path::{Path, PathBuf};
use std::string::String;

pub enum ApplyResult {
    Changed,
    Unchanged,
    Error,
}

pub struct DotFile {
    //TODO maybe implement finalize?
    specialcomments: Vec<Specialcomment>,
    pub sections: Vec<Section>,
    pub file: File,
    pub filename: String,
    pub targetfile: Option<String>,
    pub metafile: Option<MetaFile>,
    pub commentsign: String,
    pub modified: bool,
    pub permissions: Option<u32>,
}

impl DotFile {
    pub fn new(filename: &str) -> Result<DotFile, std::io::Error> {
        let filepath = PathBuf::from(filename);
        Self::from_pathbuf(&filepath)
    }

    pub fn from_pathbuf(path: &PathBuf) -> Result<DotFile, std::io::Error> {
        let sourcepath = path
            .canonicalize()
            .expect("could not canonicalize path")
            .display()
            .to_string();

        let sourcefile = match OpenOptions::new().read(true).write(true).open(path) {
            Err(e) => {
                if e.kind() == ErrorKind::PermissionDenied {
                    // open file as readonly if writing is not permitted
                    // TODO: skip readonly files entirely
                    match OpenOptions::new().read(true).write(false).open(path) {
                        Ok(file) => file,
                        Err(error) => return Err(error),
                    }
                } else {
                    return Err(e);
                }
            }
            Ok(file) => file,
        };

        let metafile;

        let mut comments = Vec::new();
        let mut line_counter = 0;

        let mut sections: Vec<Section> = Vec::new();
        let mut lines: Vec<ContentLine> = Vec::new();

        let mut comment_map: CommentMap = CommentMap::new();
        let mut section_map: HashMap<String, Vec<Specialcomment>> = HashMap::new();

        let mut target_file: Option<String> = Option::None;
        let mut permissions = Option::None;
        let mut commentsign = String::new();
        let mut hascommentsign = false;

        // check for metafile
        if Path::new(&format!("{}.imosid.toml", sourcepath)).is_file() {
            let mut content = String::new();
            io::BufReader::new(&sourcefile).read_to_string(&mut content)?;

            metafile = if let Some(mut metafile) = MetaFile::new(
                PathBuf::from(&format!("{}.imosid.toml", sourcepath)),
                &content,
            ) {
                metafile.finalize();
                metafile
            } else {
                return Err(std::io::Error::new(ErrorKind::Other, "invalid metafile"));
            };
            return Ok(DotFile {
                specialcomments: comments,
                sections,
                file: sourcefile,
                filename: sourcepath,
                targetfile: metafile.targetfile.clone(),
                modified: metafile.modified,
                permissions: metafile.permissions.clone(),
                metafile: Some(metafile),
                commentsign: String::from(""),
            });
        }

        let filelines = io::BufReader::new(&sourcefile).lines();
        // parse lines for special comments
        for i in filelines {
            line_counter += 1;
            let line = i?;
            // TODO: Do this better
            if !hascommentsign {
                commentsign = String::from(get_comment_sign(&sourcepath, &line));
                hascommentsign = true;
            }

            let newcomment = Specialcomment::from_line(&line, &commentsign, line_counter);
            match newcomment {
                Some(comment) => {
                    // comments with section all apply to the entire file
                    //TODO: move checking into comment from_line
                    comment_map.push_comment(comment.clone());
                    comments.push(comment.clone());
                }
                None => lines.push(ContentLine {
                    linenumber: line_counter,
                    content: line,
                }),
            }
        }

        comment_map.remove_incomplete();

        if let Some(comment) = comment_map.get_comment("all", CommentType::TargetInfo) {
            if let Some(arg) = &comment.argument {
                target_file = Some(String::from(arg));
            }
        }
        if let Some(comment) = comment_map.get_comment("all", CommentType::PermissionInfo) {
            if let Some(arg) = &comment.argument {
                permissions = match arg.split_at(3).1.parse::<u32>() {
                    Err(_) => Option::None,
                    Ok(permnumber) => Option::Some(permnumber),
                }
            }
        }

        for sectionname in comment_map.get_sections() {
            Section::from_comment_map(sectionname, &comment_map).map(|section| {
                sections.push(section);
            });
        }

        // sort sections by lines (retaining the original order of the file)
        sections.sort_by(|a, b| a.get_data().startline.cmp(&b.get_data().startline));

        // detect overlapping sections
        let vecsize = sections.len();
        let mut broken_indices = Vec::new();
        let mut skipnext = false;
        for i in 0..vecsize {
            if skipnext {
                skipnext = false;
                continue;
            }
            let currentsection = &sections[i];
            if i < vecsize - 1 {
                let nextsection = &sections[i + 1];
                if nextsection.get_data().startline < currentsection.get_data().endline {
                    broken_indices.push(i + 1);
                    broken_indices.push(i);
                    skipnext = true;
                }
            }
        }

        for i in broken_indices {
            println!("section {} overlapping", i);
            sections.remove(i);
        }

        let mut modified = false;
        // introduce anonymous sections
        if sections.len() > 0 {
            let mut currentline = 1;
            let mut tmpstart;
            let mut tmpend;
            let mut anonymous_sections: Vec<Section> = Vec::new();
            for i in &sections {
                if i.get_data().startline - currentline >= 1 {
                    tmpstart = currentline;
                    tmpend = i.get_data().startline - 1;
                    let newsection = Section::new_anonymous(tmpstart, tmpend);
                    anonymous_sections.push(newsection);
                }
                currentline = i.get_data().endline + 1;
            }

            sections.extend(anonymous_sections);
            sections.sort_by(|a, b| a.get_data().startline.cmp(&b.get_data().startline));
        } else {
            // make the entire file one anonymous section
            let newsection = Section::new_anonymous(1, lines.len() as u32);
            sections.push(newsection);
        }

        // fill sections with content
        for i in &mut sections {
            // TODO: speed this up, binary search or something
            for c in &lines {
                if c.linenumber > i.get_data().endline {
                    break;
                } else if c.linenumber < i.get_data().startline {
                    continue;
                }
                i.push_str(&c.content);
            }
            i.finalize();
            //TODO: deal with "modified" variable
        }

        let retfile = DotFile {
            specialcomments: comments,
            sections,
            file: sourcefile,
            filename: sourcepath,
            targetfile: target_file,
            commentsign,
            metafile: None,
            modified,
            permissions,
        };

        return Ok(retfile);
    }

    fn get_named_sections(&self) -> Vec<(&SectionData, &NamedSectionData)> {
        let mut retvec: Vec<(&SectionData, &NamedSectionData)> = Vec::new();
        for i in &self.sections {
            if let Section::Named(data, named_data) = i {
                retvec.push((&data, &named_data));
            }
        }
        return retvec;
    }

    pub fn count_named_sections(&self) -> u32 {
        let mut counter = 0;
        for i in &self.sections {
            if let Section::Named { .. } = i {
                counter += 1;
            }
        }
        counter
    }

    pub fn is_managed(&self) -> bool {
        if self.metafile.is_some() {
            return true;
        }
        if !self.is_anonymous() {
            return true;
        }
        return false;
    }

    pub fn pretty_info(&self) -> String {
        let mut retstring = String::new();
        match &self.metafile {
            Some(metafile) => {
                retstring.push_str(&metafile.pretty_info());
            } // TODO
            None => {
                retstring.push_str(&format!("comment syntax: {}\n", self.commentsign));
                for section in self.sections.iter() {
                    if let Some(section_info) = &section.pretty_info() {
                        retstring.push_str(&section_info);
                        retstring.push('\n');
                    }
                }
            }
        };
        if let Some(permissions) = self.permissions {
            retstring.push_str(&format!(
                "target permissions: {}\n",
                permissions.to_string().bold()
            ));
        }

        if let Some(targetfile) = &self.targetfile {
            retstring.push_str(&format!("target : {}\n", targetfile.to_string().bold()));
        }

        return retstring;
    }

    pub fn update(&mut self) {
        //iterate over sections in self.sections

        let mut modified = false;
        let mut applymap: HashMap<&String, DotFile> = HashMap::new();
        let mut source_sections = Vec::new();
        if self.metafile.is_some() {
            let metafile = &self.metafile.as_ref().unwrap();
            if metafile.modified {
                return;
            }
            if !metafile.sourcefile.is_some() {
                return;
            }
            //TODO look up what as_ref does
            match DotFile::new(&metafile.sourcefile.as_ref().unwrap()) {
                Ok(file) => {
                    modified = self.applyfile(&file);
                }
                Err(e) => {
                    println!("failed to apply metafile sourfe, error: {}", e);
                }
            }
            return;
        }

        for section in &self.sections {
            if let Section::Named(data, named_data) = section {
                if let Some(source) = &named_data.source {
                    if !applymap.contains_key(source) {
                        match DotFile::new(source) {
                            Ok(sfile) => {
                                applymap.insert(source, sfile);
                            }
                            Err(_) => {
                                println!("error: could not open source file {}", source);
                                continue;
                            }
                        }
                    }
                    if let Some(sfile) = applymap.get(source) {
                        source_sections.push(sfile.clone().get_section(source).unwrap());
                    }
                }
            }
        }

        for applysection in source_sections.iter() {
            if let Section::Named(data, named_data) = applysection.clone() {
                self.applysection(data, named_data);
            }
        }
    }

    fn get_section(&self, name: &str) -> Option<Section> {
        for i in &self.sections {
            if let Section::Named(_, named_data) = i {
                if named_data.name == name {
                    return Some(i.clone());
                }
            }
        }
        None
    }

    // delete section sectionname from sections
    pub fn deletesection(&mut self, sectionname: &str) -> bool {
        if let Some(index) = self.sections.iter().position(|x| match &x {
            Section::Named(_, named_data) => named_data.name.eq(sectionname),
            _ => false,
        }) {
            self.sections.remove(index);
            println!("deleting section {}", sectionname);
            return true;
        } else {
            return false;
        }
    }

    //TODO: changedstatus
    pub fn compile(&mut self) -> bool {
        let mut didsomething = false;
        match &mut self.metafile {
            None => {
                for i in 0..self.sections.len() {
                    didsomething = self.sections[i].compile().into() || didsomething;
                }
            }
            Some(metafile) => {
                didsomething = metafile.compile().into();
            }
        }
        didsomething
    }

    pub fn write_to_file(&mut self) {
        let targetname = &expand_tilde(&self.filename);
        let newfile = File::create(targetname);
        match newfile {
            Err(_) => {
                println!("error: could not write to file {}", &self.filename);
                panic!("write_to_file");
            }
            Ok(mut file) => match &mut self.metafile {
                None => {
                    file.write_all(self.to_string().as_bytes()).unwrap();
                }
                Some(metafile) => {
                    file.write_all(metafile.content.as_bytes()).unwrap();
                    metafile.write_to_file();
                }
            },
        }

        if let Some(permissions) = self.permissions {
            let mut perms = fs::metadata(targetname).unwrap().permissions();
            let permint = u32::from_str_radix(&format!("{}", permissions + 1000000), 8).unwrap();
            perms.set_mode(permint);
            println!("setting permissions");
            fs::set_permissions(targetname, perms).expect("failed to set permissions");
        }
    }

    // create the target file if not existing
    // TODO: result
    pub fn create_file(source: &DotFile) -> bool {
        let targetpath = String::from(source.targetfile.clone().unwrap());
        let realtargetpath = expand_tilde(&targetpath);
        // create new file
        match &source.metafile {
            None => {
                let mut targetfile: DotFile = DotFile {
                    specialcomments: source.specialcomments.clone(),
                    sections: source.sections.clone(),
                    filename: realtargetpath.clone(),
                    targetfile: Option::Some(targetpath),
                    commentsign: source.commentsign.clone(),
                    file: source.file.try_clone().unwrap(),
                    metafile: None,
                    modified: source.modified,
                    permissions: source.permissions,
                };
                targetfile.write_to_file();
                return true;
            }
            Some(metafile) => {
                if metafile.modified {
                    println!(
                        "{}",
                        format!("{} modified, skipping", &source.filename).yellow()
                    );
                    return false;
                }
                OpenOptions::new()
                    .write(true)
                    .open(&realtargetpath)
                    .expect(&format!("cannot open file {}", &targetpath))
                    .write_all(metafile.content.as_bytes())
                    .expect(&format!("could not write file {}", &targetpath));
                let mut newmetafile = MetaFile::from(PathBuf::from(&realtargetpath));
                newmetafile.sourcefile = Some(source.filename.clone());
                newmetafile.permissions = metafile.permissions;
                newmetafile.write_to_file();
                newmetafile.write_permissions();
                return true;
            }
        }
    }

    pub fn is_anonymous(&self) -> bool {
        self.count_named_sections() > 0
    }

    pub fn apply(&self) -> ApplyResult {
        let mut donesomething = false;
        if let Some(target) = &self.targetfile {
            if create_file(&target) {
                if DotFile::create_file(self) {
                    println!(
                        "applied {} to create {} ",
                        &self.filename.green(),
                        &target.bold()
                    );
                    donesomething = true;
                }
            } else {
                let mut targetfile = match DotFile::new(&expand_tilde(&target)) {
                    Ok(file) => file,
                    Err(_) => {
                        eprintln!("failed to parse {}", &target.red());
                        return ApplyResult::Error;
                    }
                };
                if targetfile.applyfile(&self) {
                    println!("applied {} to {} ", &self.filename.green(), &target.bold());
                    targetfile.write_to_file();
                    donesomething = true;
                }
            }
        } else {
            println!("{} has no target file", &self.filename.red());
            return ApplyResult::Error;
        }
        if donesomething {
            return ApplyResult::Changed;
        } else {
            return ApplyResult::Unchanged;
        }
    }

    fn can_apply(&self, other: &DotFile) -> bool {
        if self.metafile.is_some() {
            if other.metafile.is_some() {
                return true;
            } else {
                eprintln!(
                    "{} {}",
                    "cannot apply comment file to metafile ".yellow(),
                    self.filename.yellow().bold()
                );
                return false;
            }
        } else {
            if self.is_anonymous() {
                eprintln!(
                    "{} {}",
                    "cannot apply to unmanaged file ".yellow(),
                    self.filename.yellow().bold()
                );
                return false;
            }
            if other.metafile.is_some() {
                eprintln!(
                    "cannot apply metafile to normal imosid file {}",
                    self.filename.bold()
                );
                return false;
            } else {
                if other.is_anonymous() {
                    eprintln!("{} {}", other.filename.red(), "is unmanaged, cannot apply");
                    return false;
                } else {
                    return true;
                }
            }
        }
    }

    fn has_section(&self, name: &str) -> bool {
        for (_, named_data) in self.get_named_sections() {
            if named_data.name == name {
                return true;
            }
        }
        return false;
    }

    fn has_same_sections(&self, other: &DotFile) -> bool {
        if self.sections.len() != other.sections.len() {
            return false;
        }
        for (_, named_data) in self.get_named_sections() {
            if !other.has_section(&named_data.name) {
                return false;
            }
        }
        return true;
    }

    // return true if file will be modified
    // applies other file to self
    // TODO: return result
    pub fn applyfile(&mut self, inputfile: &DotFile) -> bool {
        if !self.can_apply(inputfile) {
            return false;
        }
        match &mut self.metafile {
            None => {
                //if no sections are updated, don't write anything to the file system
                let mut modified = false;

                // true if input file contains all sections that self has
                let mut allsections = self.has_same_sections(&inputfile);

                if !self.modified && allsections {
                    // copy entire file contents if all sections are unmodified
                    self.sections = inputfile.sections.clone();
                    self.specialcomments = inputfile.specialcomments.clone();
                    println!(
                        "applied all sections from {} to {}",
                        inputfile.filename.bold(),
                        self.filename.bold()
                    );
                    modified = true;
                } else {
                    let mut applycounter = 0;
                    for (data, named_data) in inputfile.get_named_sections() {
                        if self.applysection(data.clone(), named_data.clone()) {
                            applycounter += 1;
                            modified = true;
                        }
                    }
                    if modified {
                        println!(
                            "applied {} sections from {} to {}",
                            applycounter,
                            inputfile.filename.bold(),
                            self.filename.bold()
                        );
                    } else {
                        println!(
                            "applied no sections from {} to {}{}",
                            inputfile.filename.bold().dimmed(),
                            self.filename.bold().dimmed(),
                            if self.modified {
                                " (modified)".dimmed()
                            } else {
                                "".dimmed()
                            }
                        );
                    }
                }
                return modified;
            }

            // apply entire content if file is managed by metafile
            Some(metafile) => {
                if !metafile.modified {
                    if let Some(applymetafile) = &inputfile.metafile {
                        if applymetafile.modified {
                            println!("source file {} modified", &applymetafile.parentfile);
                            return false;
                        }
                        if metafile.hash == applymetafile.hash {
                            println!("file {} already up to date", self.filename.bold());
                            return false;
                        }
                        metafile.content = applymetafile.content.clone();
                        metafile.hash = applymetafile.hash.clone();

                        println!(
                            "applied {} to {}",
                            inputfile.filename.bold(),
                            self.filename.bold()
                        );
                        return true;
                    }
                } else {
                    println!(
                        "{}",
                        format!("target {} modified, skipping", &self.filename.bold()).yellow()
                    );
                }
                return false;
            }
        }
    }

    fn applysection(&mut self, sectiondata: SectionData, named_data: NamedSectionData) -> bool {
        if let Some(_) = &self.metafile {
            eprintln!(
                "{}",
                "cannot apply individual section to file managed by metafile"
                    .red()
                    .bold()
            );
            return false;
        }
        if named_data.hash != named_data.targethash {
            eprintln!("cannot apply modified section");
            return false;
        }

        for section_index in 0..self.sections.len() {
            let tmpsection = self.sections.get(section_index).unwrap();
            if let Section::Named(src_data, src_named_data) = tmpsection {
                if src_named_data.name.eq(&named_data.name) {
                    self.sections[section_index] = Section::Named(sectiondata, named_data);
                    return true;
                }
            }
        }
        return false;
    }

    pub fn get_hashbang(&self) -> Option<String> {
        let firstsection = self.sections.get(0).unwrap();
        if let Section::Anonymous(section_data) = firstsection {
            let firstline = section_data.content.split("\n").nth(0).unwrap();
            if Regex::new("^#!/.*").unwrap().is_match(&firstline) {
                return Some(String::from(firstline));
            }
        }
        None
    }

    fn get_property_comments(&self) -> String {
        let mut retstr = String::new();
        // TODO: do same thing with all "all" section comments
        if let Some(targetfile) = &self.targetfile {
            retstr.push_str(&Specialcomment::new_string(
                &self.commentsign,
                CommentType::TargetInfo,
                "all",
                None,
            ));
            retstr.push_str("\n");
        }

        retstr
    }
}

impl ToString for DotFile {
    fn to_string(&self) -> String {
        match &self.metafile {
            None => {
                let mut retstr = String::new();
                let outputsections;

                // respect hashbang
                // and put comments below it
                match self.get_hashbang() {
                    Some(hashbang) => {
                        retstr.push_str(&format!("{}\n", hashbang));
                        retstr.push_str(&self.get_property_comments());
                        retstr.push_str(
                            &self
                                .sections
                                .get(0)
                                .unwrap()
                                .get_data()
                                .content
                                .lines()
                                .collect::<Vec<&str>>()[1..]
                                .join("\n"),
                        );
                        outputsections = &self.sections[1..];
                    }
                    None => {
                        retstr.push_str(&self.get_property_comments());
                        outputsections = &self.sections[..];
                    }
                }

                for i in outputsections {
                    retstr.push_str(&i.output(&self.commentsign));
                }
                return retstr;
            }
            Some(metafile) => {
                return metafile.content.clone();
            }
        }
    }
}

// detect comment syntax for file based on filename, extension and hashbang
fn get_comment_sign(filename: &str, firstline: &str) -> String {
    let fpath = Path::new(filename);

    let file_name_commentsigns: HashMap<&str, &str> = HashMap::from([
        ("dunstrc", "#"),
        ("jgmenurc", "#"),
        ("zshrc", "#"),
        ("bashrc", "#"),
        ("Xresources", "!"),
        ("xsettingsd", "#"),
        ("vimrc", "\""),
    ]);

    // get comment syntax via file name
    let fname = fpath.file_name().and_then(OsStr::to_str);
    match fname {
        Some(name) => {
            let filename = String::from(String::from(name).trim_start_matches("."));
            match file_name_commentsigns.get(filename.as_str()) {
                Some(sign) => {
                    return String::from(sign.deref());
                }
                None => {}
            }
        }
        None => {}
    }

    let mut file_type_commentsigns: HashMap<&str, &str> = HashMap::from([
        ("py", "#"),
        ("sh", "#"),
        ("zsh", "#"),
        ("bash", "#"),
        ("fish", "#"),
        ("c", "//"),
        ("cpp", "//"),
        ("rasi", "//"),
        ("desktop", "#"),
        ("conf", "#"),
        ("vim", "\""),
        ("reg", ";"),
        ("rc", "#"),
        ("ini", ";"),
        ("xresources", "!"),
    ]);

    let ext = fpath.extension().and_then(OsStr::to_str);

    // get comment syntax via file extension
    match ext {
        Some(extension) => {
            let tester = file_type_commentsigns.get(extension);
            match tester {
                Some(sign) => {
                    return String::from(sign.deref());
                }
                None => {}
            }
        }
        None => {}
    }

    // get comment syntax via #!/hashbang

    let mut file_hashbang_commentsigns: HashMap<&str, &str> = HashMap::from([
        ("python", "#"),
        ("sh", "#"),
        ("bash", "#"),
        ("zsh", "#"),
        ("fish", "#"),
        ("node", "//"),
    ]);

    match Regex::new("^#!/.*[/ ](.*)$").unwrap().captures(&firstline) {
        Some(captures) => {
            let application = captures.get(1).unwrap().as_str();
            match file_hashbang_commentsigns.get(application) {
                Some(sign) => {
                    return String::from(sign.deref());
                }
                None => {}
            }
        }
        None => {}
    }

    return String::from("#");
}

// expand tilde in path into the home folder
pub fn expand_tilde(input: &str) -> String {
    let mut retstr = String::from(input);
    if retstr.starts_with("~/") {
        retstr = String::from(format!(
            "{}/{}",
            home::home_dir()
                .unwrap()
                .into_os_string()
                .into_string()
                .unwrap(),
            retstr.strip_prefix("~/").unwrap()
        ));
    }
    return retstr;
}

// create file with directory creation and
// parsing of the home tilde
// MAYBETODO: support environment variables
// return false if file already exists
pub fn create_file(path: &str) -> bool {
    let realtargetname = expand_tilde(path);

    let checkpath = Path::new(&realtargetname);
    if !checkpath.is_file() {
        let bufpath = checkpath.to_path_buf();
        match bufpath.parent() {
            Some(parent) => {
                std::fs::create_dir_all(parent.to_str().unwrap()).unwrap();
            }
            None => {}
        }
        File::create(&realtargetname).unwrap();
        return true;
    } else {
        return false;
    }
}
