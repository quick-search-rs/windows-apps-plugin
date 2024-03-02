// NOTE: UWP APPS ARE A FEATURE FLAG BECAUSE I CAN'T LIST THEM WITHOUT ADMINISTRATOR RIGHTS BUT IF WE HAVE ADMINISTRATOR RIGHTS THEN OPENING FILES DOESN'T WORK

use std::{env::VarError, str::FromStr};

use abi_stable::{
    export_root_module,
    prefix_type::PrefixTypeTrait,
    sabi_extern_fn,
    sabi_trait::prelude::TD_Opaque,
    std_types::{RBox, RStr, RString, RVec},
};
use quick_search_lib::{ColoredChar, Log, PluginId, SearchLib, SearchLib_Ref, SearchResult, Searchable, Searchable_TO};

static NAME: &str = "Windows Apps";

#[export_root_module]
pub fn get_library() -> SearchLib_Ref {
    if std::env::var("RUST_LOG").map(|s| s == "trace").unwrap_or(false) {
        env_logger::init();
    }
    SearchLib { get_searchable }.leak_into_prefix()
}

#[sabi_extern_fn]
fn get_searchable(id: PluginId, logger: quick_search_lib::ScopedLogger) -> Searchable_TO<'static, RBox<()>> {
    let this = WindowsApps::new(id, logger);
    Searchable_TO::from_value(this, TD_Opaque)
}

struct WindowsApps {
    id: PluginId,
    config: quick_search_lib::Config,
    logger: quick_search_lib::ScopedLogger,
}

impl WindowsApps {
    pub fn new(id: PluginId, logger: quick_search_lib::ScopedLogger) -> Self {
        Self {
            id,
            config: get_default_config(),
            logger,
        }
    }
}

impl Searchable for WindowsApps {
    fn search(&self, query: RString) -> RVec<SearchResult> {
        let mut res: Vec<SearchResult> = vec![];
        let query = query.to_lowercase();

        #[cfg(feature = "uwp")]
        let include_uwp_apps = self.config.get("Include UWP Apps in search results").and_then(|v| v.as_bool()).unwrap_or(true);
        let include_start_menu_apps = self.config.get("Include Start Menu Apps in search results").and_then(|v| v.as_bool()).unwrap_or(true);
        let return_error_messages = self.config.get("Return Error messages").and_then(|v| v.as_bool()).unwrap_or(false);

        #[cfg(feature = "uwp")]
        if include_uwp_apps {
            insert_uwp_apps(&query, return_error_messages, &mut res)
        }

        if include_start_menu_apps {
            if let Err(e) = insert_start_menu_apps(&query, return_error_messages, &mut res) {
                self.logger.error(&format!("failed to insert start menu apps: {}", e));
                if return_error_messages {
                    res.push(SearchResult::new("failed to insert start menu apps").set_context(&format!("{}", e)));
                }
            }
        }
        res.sort_by(|a, b| a.title().cmp(b.title()));
        res.dedup_by(|a, b| a.title() == b.title());

        res.into()
    }
    fn name(&self) -> RStr<'static> {
        NAME.into()
    }
    fn colored_name(&self) -> RVec<quick_search_lib::ColoredChar> {
        // can be dynamic although it's iffy how it might be used
        vec![
            ColoredChar::new_rgba('W', 115, 115, 115, 255),
            ColoredChar::new_rgba('i', 115, 115, 115, 255),
            ColoredChar::new_rgba('n', 115, 115, 115, 255),
            ColoredChar::new_rgba('d', 115, 115, 115, 255),
            ColoredChar::new_rgba('o', 115, 115, 115, 255),
            ColoredChar::new_rgba('w', 115, 115, 115, 255),
            ColoredChar::new_rgba('s', 115, 115, 115, 255),
            ColoredChar::new_rgba(' ', 115, 115, 115, 255),
            ColoredChar::new_rgba('A', 242, 80, 34, 255),
            ColoredChar::new_rgba('p', 127, 186, 0, 255),
            ColoredChar::new_rgba('p', 0, 164, 239, 255),
            ColoredChar::new_rgba('s', 255, 185, 0, 255),
        ]
        .into()
    }
    fn execute(&self, result: &SearchResult) {
        // let s = result.extra_info();
        // if let Ok::<clipboard::ClipboardContext, Box<dyn std::error::Error>>(mut clipboard) = clipboard::ClipboardProvider::new() {
        //     if let Ok(()) = clipboard::ClipboardProvider::set_contents(&mut clipboard, s.to_owned()) {
        //         println!("copied to clipboard: {}", s);
        //     } else {
        //         println!("failed to copy to clipboard: {}", s);
        //     }
        // } else {
        //     self.logger.error!("failed to copy to clipboard: {}", s);
        // }

        // finish up, above is a clipboard example

        // determine if it's a "pth:..." or a "uwp:..." and handle accordingly
        let mut extra_info = result.extra_info().split(':');
        let prefix = match extra_info.next() {
            Some(p) => p,
            None => {
                self.logger.error("failed to get prefix from extra_info");
                return;
            }
        };

        let extra_info = extra_info.collect::<Vec<&str>>().join(":");

        match prefix {
            "pth" => {
                self.logger.info(&format!("opening file: {}", extra_info));

                let path = {
                    match std::path::PathBuf::from_str(&extra_info) {
                        Ok(p) => p,
                        Err(e) => {
                            self.logger.error(&format!("failed to get path: {}", e));
                            return;
                        }
                    }
                };

                self.logger.trace(&format!("path: {:?}", path));

                if let Err(e) = opener::open(path) {
                    self.logger.error(&format!("failed to open file: {}", e));
                } else {
                    self.logger.info("opened file");
                }
            }
            #[cfg(feature = "uwp")]
            "uwp" => {
                self.logger.info(&format!("opening UWP app: {}", extra_info));
                // open UWP app
            }
            prefix => {
                self.logger.error(&format!("unknown prefix: {}", prefix));
            }
        }
    }
    fn plugin_id(&self) -> PluginId {
        self.id.clone()
    }
    fn get_config_entries(&self) -> quick_search_lib::Config {
        get_default_config()
    }
    fn lazy_load_config(&mut self, config: quick_search_lib::Config) {
        self.config = config;
    }
}

fn insert_start_menu_apps(query: &str, return_error_messages: bool, res: &mut Vec<SearchResult>) -> Result<(), VarError> {
    // enumerate recursively through the start menu folder: C:\ProgramData\Microsoft\Windows\Start Menu
    // find all .lnk files whos names do not contain "uninstall" (when lowercase)
    // then using the name of the .lnk file (minus the extension) as well as the path to the .lnk file
    // we can construct a SearchResult, with the title being the name, the context being the path to the .lnk file

    let mut entries = vec![];

    let start_menu_path = std::path::PathBuf::from(r"C:\ProgramData\Microsoft\Windows\Start Menu");
    recursively_read_folder_for_links(&start_menu_path, &mut entries);

    let appdata = std::env::var("APPDATA")?;
    // %appdata%\Microsoft\Windows\Start Menu\Programs
    let mut path = std::path::PathBuf::from(appdata);
    path.push("Microsoft");
    path.push("Windows");
    path.push("Start Menu");
    path.push("Programs");
    recursively_read_folder_for_links(&path, &mut entries);

    // filter out uninstall links and invalid links, as well as links that do not contain the query
    entries.retain(|p| {
        p.file_name()
            .and_then(|f| f.to_str())
            .map(|f| !f.to_lowercase().contains("uninstall") && f.to_lowercase().contains(query))
            .unwrap_or(false)
    });
    // sort and dedup
    entries.sort();
    entries.dedup();

    for entry in entries {
        let title = match entry.file_stem().and_then(|f| f.to_str()) {
            Some(f) => f,
            None => {
                if return_error_messages {
                    res.push(SearchResult::new("failed to get file_stem").set_context(&format!("{:?}", entry)));
                }
                continue;
            }
        };
        let context = match entry.to_str() {
            Some(s) => s,
            None => {
                if return_error_messages {
                    res.push(SearchResult::new("failed to get path").set_context(&format!("{:?}", entry)));
                }
                continue;
            }
        };
        res.push(SearchResult::new(title).set_context(context).set_extra_info(&(String::from("pth:") + context)));
    }
    Ok(())
}

#[cfg(feature = "uwp")]
fn insert_uwp_apps(query: &str, return_error_messages: bool, res: &mut Vec<SearchResult>) {
    if !is_elevated::is_elevated() {
        self.logger.error(&format!("no administrator rights"));
        if return_error_messages {
            res.push(SearchResult::new("no administrator rights").set_context("try running as administrator"));
        }
        return;
    }

    let pacman = match windows::Management::Deployment::PackageManager::new() {
        Ok(p) => p,
        Err(e) => {
            self.logger.error(&format!("failed to get PackageManager: {}", e));
            if return_error_messages {
                res.push(SearchResult::new("failed to get PackageManager from windows_api").set_context(&format!("{}", e)));
            }
            return;
        }
    };

    let packages = match pacman.FindPackages() {
        Ok(p) => p,
        Err(e) => {
            self.logger.error(&format!("failed to get packages: {}", e));
            if return_error_messages {
                res.push(SearchResult::new("failed to get packages from PackageManager").set_context(&format!("{}", e)));
            }
            return;
        }
    };

    for package in packages {
        let name = match package.DisplayName().map(|d| d.to_string()) {
            Ok(n) => n,
            Err(e) => {
                self.logger.error(&format!("failed to get DisplayName: {}", e));
                if return_error_messages {
                    res.push(SearchResult::new("failed to get DisplayName from package").set_context(&format!("{}", e)));
                }
                continue;
            }
        };

        if !name.to_lowercase().contains(query) {
            continue;
        }

        let description = match package.Description().map(|d| d.to_string()) {
            Ok(d) => d,
            Err(e) => {
                self.logger.error(&format!("failed to get Description: {}", e));
                if return_error_messages {
                    res.push(SearchResult::new("failed to get Description from package").set_context(&format!("{}", e)));
                }
                continue;
            }
        };

        res.push(SearchResult::new(&name).set_context(&description).set_extra_info(&(String::from("uwp:") + &name)));
    }
}

fn get_default_config() -> quick_search_lib::Config {
    let mut config = quick_search_lib::Config::new();
    #[cfg(feature = "uwp")]
    config.insert("Include UWP Apps in search results".into(), quick_search_lib::EntryType::Bool { value: true });
    config.insert("Include Start Menu Apps in search results".into(), quick_search_lib::EntryType::Bool { value: true });
    config.insert("Return Error messages".into(), quick_search_lib::EntryType::Bool { value: false });
    config
}

fn recursively_read_folder_for_links(path: &std::path::PathBuf, found: &mut Vec<std::path::PathBuf>) {
    if let Ok(entries) = std::fs::read_dir(path) {
        entries.for_each(|entry| {
            if let Ok(entry) = entry {
                if let Ok(metadata) = entry.metadata() {
                    if metadata.is_dir() {
                        recursively_read_folder_for_links(&entry.path(), found);
                    } else if metadata.is_file() {
                        if let Some(ext) = entry.path().extension() {
                            if ext == "lnk" {
                                found.push(entry.path());
                            }
                        }
                    }
                }
            }
        });
    }
}
