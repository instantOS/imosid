use crate::built_info;
use crate::hashable::{ChangeState, Hashable};
use colored::Colorize;
use semver::Version;
use sha256::digest;
use std::fs::{self, read_to_string, File};
use std::io::Write;
use std::os::unix::prelude::PermissionsExt;
use std::path::PathBuf;
use toml::Value;

// a file containing metadata about an imosid file for file types which do not support comments
pub struct MetaFile {
    currenthash: String,
    pub hash: String,
    pub parentfile: String,
    pub targetfile: Option<String>,
    pub sourcefile: Option<String>,
    pub modified: bool,
    imosidversion: Version,
    syntaxversion: i64,
    value: Value,
    pub content: String,
    path: PathBuf,
    pub permissions: Option<u32>,
}

impl Hashable for MetaFile {
    // check for modifications
    fn finalize(&mut self) {
        self.currenthash = self.get_content_hash();
        self.modified = self.hash != self.currenthash;
    }

    fn compile(&mut self) -> ChangeState {
        let contenthash = self.get_content_hash();
        self.modified = false;
        if self.hash == contenthash {
            ChangeState::Unchanged
        } else {
            self.hash = contenthash;
            ChangeState::Changed
        }
    }
}

impl MetaFile {
    //TODO: Result
    //TODO: serde DTO
    pub fn new(path: PathBuf, content: &str) -> Option<MetaFile> {
        let mcontent = read_to_string(&path).unwrap();
        let value = mcontent.parse::<Value>().expect("failed to read toml");

        //TODO: fileinfo struct for fields in both dotfile and metafile
        let mut retfile = MetaFile {
            currenthash: String::from(""),
            targetfile: None,
            sourcefile: None,
            hash: String::from(""),
            parentfile: String::from(""),
            // default version strings
            imosidversion: Version::new(0, 0, 0),
            syntaxversion: 1,
            value: value.clone(),
            content: String::from(content),
            modified: false,
            permissions: Option::None,
            path,
        };

        // hash and parent are mandatory
        retfile.hash = value.get("hash")?.as_str()?.to_string();
        retfile.parentfile = value.get("parent")?.as_str()?.to_string();

        if let Some(Value::String(targetfile)) = value.get("target") {
            retfile.targetfile = Some(String::from(targetfile));
        }

        if let Some(Value::String(sourcefile)) = value.get("source") {
            retfile.sourcefile = Some(String::from(sourcefile));
        }

        if let Some(Value::Integer(permissions)) = value.get("permissions") {
            //TODO check if permissions smaller than 777
            retfile.permissions = Some(*permissions as u32);
        }

        if let Some(Value::Integer(syntaxversion)) = value.get("syntaxversion") {
            retfile.syntaxversion = syntaxversion.clone();
        }

        if let Some(Value::String(imosidversion)) = value.get("imosidversion") {
            if let Ok(version) = Version::parse(imosidversion) {
                retfile.imosidversion = version;
            }
        }

        Some(retfile)
    }

    fn get_parent_file(&self) -> PathBuf {
        let mut path = self.path.clone();
        path.push(&self.parentfile);
        path.pop();
        path
    }

    // TODO incorporate this into normal write
    pub fn write_permissions(&self) {
        let parentpath = self.get_parent_file();
        if let Some(permissions) = &self.permissions {
            let mut perms = fs::metadata(&parentpath).unwrap().permissions();
            let permint = u32::from_str_radix(&format!("{}", permissions + 1000000), 8).unwrap();
            perms.set_mode(permint);
            fs::set_permissions(&parentpath, perms).expect("failed to set permissions");
        } else {
            println!("no permissions");
        }
    }

    // create a new metafile for a file
    // TODO maybe return result?
    // TODO split this up, this doesn't need to write to disk
    pub fn from(sourcepath: PathBuf) -> MetaFile {
        let mut path = sourcepath.clone();
        //
        //TODO handle result
        let filecontent =
            read_to_string(&path).expect("could not read file content to create metafile");

        let parentname = path
            .file_name()
            .unwrap()
            .to_os_string()
            .into_string()
            .unwrap();

        //TODO don't create metafiles for metafiles
        let filename = format!("{}.imosid.toml", parentname);

        path.pop();
        path.push(filename);

        let mut retfile: MetaFile;
        //Maybe distinguish between new and from path?
        if path.is_file() {
            retfile = MetaFile::new(path.clone(), &filecontent).expect("could not create metafile");
            retfile.update();
            retfile.finalize();
        } else {
            retfile = MetaFile {
                currenthash: String::from(""),
                targetfile: None,
                sourcefile: None,
                hash: String::from(""),
                parentfile: String::from(&parentname),
                imosidversion: Version::parse(built_info::PKG_VERSION).unwrap(),
                syntaxversion: 0,
                value: Value::Integer(0),
                content: String::from(&filecontent),
                modified: false,
                permissions: Option::None,
                path,
            };

            retfile.update();
            retfile.compile();
            retfile.write_to_file();
        }

        retfile
    }

    fn get_content_hash(&self) -> String {
        digest(self.content.clone()).to_uppercase()
    }

    // populate toml value with data
    fn update(&mut self) {
        let mut selfmap = toml::map::Map::new();
        selfmap.insert("hash".into(), Value::String((&self.hash).to_string()));
        selfmap.insert("parent".into(), Value::String((&self.parentfile).into()));

        if let Some(targetfile) = &self.targetfile {
            selfmap.insert(
                String::from("target"),
                Value::String(targetfile.to_string()),
            );
        }
        if let Some(sourcefile) = &self.sourcefile {
            selfmap.insert(
                String::from("source"),
                Value::String(String::from(sourcefile)),
            );
        }

        // TODO: store syntax version somewhere central
        selfmap.insert(String::from("syntaxversion"), Value::Integer(0));

        selfmap.insert(
            String::from("imosidversion"),
            Value::String(self.imosidversion.to_string()),
        );

        selfmap.insert(
            String::from("syntaxversion"),
            Value::String(self.syntaxversion.to_string()),
        );
        self.value = Value::Table(selfmap);
    }

    pub fn output(&mut self) -> String {
        self.update();
        self.value.to_string()
    }

    pub fn write_to_file(&mut self) {
        let newfile = File::create(&self.path);
        match newfile {
            Err(_) => {
                eprintln!("{}", "Error: could not write metafile".red());
            }
            Ok(mut file) => {
                file.write_all(self.output().as_bytes())
                    .expect("could not write metafile");
            }
        }
    }

    pub fn pretty_info(&self) -> String {
        let mut ret = String::new();
        ret.push_str(&format!("metafile hash: {}\n", self.hash));
        if self.modified {
            ret.push_str(&"modified".red().bold());
        } else {
            ret.push_str(&"unmodified".green().bold());
        }
        ret
    }
}
