use std::collections::HashMap;
use std::fs;
use std::fs::File;
use std::io::{self, Write};
use std::path::Path;
use serde::Deserialize;
use clap::{Arg, Command, ArgAction};
use ignore::gitignore::{Gitignore, GitignoreBuilder};

#[derive(Debug, Deserialize)]
struct Config {
    exclude_dirs: Vec<String>,
    exclude_files: Vec<String>,
    exclude_extensions: Vec<String>,
    max_file_size: Option<u64>,
    extension_mapping: Option<HashMap<String, String>>,
}

#[derive(Debug)]
struct AppConfig {
    target_dir: String,
    output_file: Option<String>,
    include_tree: bool,
    include_contents: bool,
    print_to_console: bool,
    no_gitignore: bool,
}

#[derive(PartialEq)]
enum SkipReason {
    NoSkip,
    Skip,
    SkipWithEllipsis,
}

fn main() -> std::io::Result<()> {
    let matches = Command::new(env!("CARGO_PKG_NAME"))
        .about(env!("CARGO_PKG_DESCRIPTION"))
        .version(env!("CARGO_PKG_VERSION"))
        .author(env!("CARGO_PKG_AUTHORS"))
        .arg(
            Arg::new("directory")
                .help("Целевая директория для анализа")
                .default_value(".")
                .index(1),
        )
        .arg(
            Arg::new("output")
                .help("Выходной файл")
                .short('o')
                .long("output")
                .value_name("FILE"),
        )
        .arg(
            Arg::new("no-tree")
                .help("Не выводить дерево файлов")
                .short('T')
                .long("no-tree")
                .action(ArgAction::SetTrue),
        )
        .arg(
            Arg::new("no-contents")
                .help("Не выводить содержимое файлов")
                .short('C')
                .long("no-contents")
                .action(ArgAction::SetTrue),
        )
        .arg(
            Arg::new("print")
                .help("Вывести результат в консоль")
                .short('p')
                .long("print")
                .action(ArgAction::SetTrue),
        )
        .arg(
            Arg::new("no-gitignore")
                .help("Не учитывать правила из .gitignore")
                .short('G')
                .long("no-gitignore")
                .action(ArgAction::SetTrue),
        )
        .get_matches();

    let app_config = AppConfig {
        target_dir: matches.get_one::<String>("directory").unwrap().to_string(),
        output_file: matches.get_one::<String>("output").map(|s| s.to_string()),
        include_tree: !matches.get_flag("no-tree"),
        include_contents: !matches.get_flag("no-contents"),
        print_to_console: matches.get_flag("print"),
        no_gitignore: matches.get_flag("no-gitignore"),
    };

    if !Path::new(&app_config.target_dir).exists() || !Path::new(&app_config.target_dir).is_dir() {
        eprintln!("Ошибка: '{}' не является существующей директорией", app_config.target_dir);
        std::process::exit(1);
    }

    let config = load_builtin_config();
    
    let gitignore_matcher = if !app_config.no_gitignore {
        match create_gitignore_matcher(&app_config.target_dir) {
            Ok(matcher) => {
                println!("Учтены правила из .gitignore");
                Some(matcher)
            }
            Err(e) => {
                eprintln!("Предупреждение: {}", e);
                None
            }
        }
    } else {
        println!("Игнорирование .gitignore отключено");
        None
    };
    
    let base_dir = Path::new(&app_config.target_dir);
    
    if app_config.print_to_console {
        let mut stdout = io::stdout();
        write_markdown_format(base_dir, &mut stdout, &app_config, &config, &gitignore_matcher)?;
    } else {
        let output_file = if let Some(file) = &app_config.output_file {
            file.clone()
        } else {
            let path = base_dir.join("tree.md");
            path.to_string_lossy().to_string()
        };
        
        let mut file = File::create(&output_file)?;
        write_markdown_format(base_dir, &mut file, &app_config, &config, &gitignore_matcher)?;
        println!("Результат сохранен в файл: {}", output_file);
    }
    
    println!("Проанализирована директория: {}", app_config.target_dir);
    Ok(())
}

fn create_gitignore_matcher(dir: &str) -> Result<Gitignore, Box<dyn std::error::Error>> {
    let dir_path = Path::new(dir);
    
    let mut builder = GitignoreBuilder::new(dir_path);
    
    let gitignore_path = dir_path.join(".gitignore");
    if gitignore_path.exists() {
        builder.add(&gitignore_path);
        Ok(builder.build()?)
    } else {
        Err("Файл .gitignore не найден".into())
    }
}

fn load_builtin_config() -> Config {
    let cargo_toml_content = include_str!("../Cargo.toml");
    
    match toml::from_str::<toml::Value>(&cargo_toml_content) {
        Ok(cargo_toml) => {
            if let Some(metadata) = cargo_toml.get("package").and_then(|p| p.get("metadata")) {
                if let Some(proj2tree_config) = metadata.get("proj2tree") {
                    match proj2tree_config.clone().try_into() {
                        Ok(config) => return config,
                        Err(e) => eprintln!("Ошибка парсинга встроенной конфигурации: {}", e),
                    }
                }
            }
        }
        Err(e) => eprintln!("Ошибка парсинга встроенного Cargo.toml: {}", e),
    }

    println!("Встроенная конфигурация не найдена, используются пустые исключения");
    Config {
        exclude_dirs: Vec::new(),
        exclude_files: Vec::new(),
        exclude_extensions: Vec::new(),
        max_file_size: None,
        extension_mapping: None,
    }
}

fn write_markdown_format<W: Write>(
    base_dir: &Path,
    writer: &mut W, 
    app_config: &AppConfig, 
    config: &Config,
    gitignore_matcher: &Option<Gitignore>,
) -> std::io::Result<()> {
    let display_dir = if base_dir == Path::new(".") {
        "текущая директория".to_string()
    } else {
        base_dir.to_string_lossy().to_string()
    };
    
    writeln!(writer, "# Структура проекта: {}\n", display_dir)?;
    
    if app_config.include_tree {
        writeln!(writer, "## Дерево файлов\n")?;
        writeln!(writer, "```")?;
        print_directory_tree(base_dir, base_dir, writer, 0, app_config, config, gitignore_matcher)?;
        writeln!(writer, "```\n")?;
    }
    
    if app_config.include_contents {
        writeln!(writer, "## Содержимое файлов\n")?;
        print_file_contents_recursive(base_dir, base_dir, writer, app_config, config, gitignore_matcher)?;
    }
    
    Ok(())
}

fn print_directory_tree<W: Write>(
    base_dir: &Path,
    current_dir: &Path, 
    writer: &mut W, 
    depth: usize, 
    app_config: &AppConfig, 
    config: &Config,
    gitignore_matcher: &Option<Gitignore>,
) -> std::io::Result<()> {
    let entries = fs::read_dir(current_dir)?;
    let mut entries: Vec<_> = entries.collect::<Result<_, _>>()?;
    
    entries.sort_by_key(|a| a.file_name());
    
    for (i, entry) in entries.iter().enumerate() {
        let path = entry.path();
        let name = path.file_name().unwrap().to_string_lossy();
        
        let skip_reason = should_skip_entry(&path, name.as_ref(), app_config, config, gitignore_matcher);
        
        match skip_reason {
            SkipReason::Skip => continue,
            SkipReason::SkipWithEllipsis => {
                let prefix = if i == entries.len() - 1 { "└── " } else { "├── " };
                let indent = "    ".repeat(depth);
                write!(writer, "{}{}{}/", indent, prefix, name)?;
                writeln!(writer, " ...")?;
                continue;
            }
            SkipReason::NoSkip => {
                let prefix = if i == entries.len() - 1 { "└── " } else { "├── " };
                let indent = "    ".repeat(depth);
                
                write!(writer, "{}{}{}", indent, prefix, name)?;
                
                if path.is_dir() {
                    writeln!(writer, "/")?;
                    print_directory_tree(base_dir, &path, writer, depth + 1, app_config, config, gitignore_matcher)?;
                } else {
                    writeln!(writer)?;
                }
            }
        }
    }
    
    Ok(())
}

fn print_file_contents_recursive<W: Write>(
    base_dir: &Path,
    current_dir: &Path, 
    writer: &mut W, 
    app_config: &AppConfig, 
    config: &Config,
    gitignore_matcher: &Option<Gitignore>,
) -> std::io::Result<()> {
    let entries = fs::read_dir(current_dir)?;
    
    for entry in entries {
        let entry = entry?;
        let path = entry.path();
        let name = path.file_name().unwrap().to_string_lossy();
        
        if should_skip_entry(&path, name.as_ref(), app_config, config, gitignore_matcher) != SkipReason::NoSkip {
            continue;
        }
        
        if path.is_dir() {
            print_file_contents_recursive(base_dir, &path, writer, app_config, config, gitignore_matcher)?;
        } else {
            if is_binary_file(&path, config) || is_file_too_large(&path, config) {
                continue;
            }
            
            let relative_path = if let Ok(rel_path) = path.strip_prefix(base_dir) {
                if rel_path.as_os_str().is_empty() {
                    Path::new(".").join(name.as_ref())
                } else {
                    rel_path.to_path_buf()
                }
            } else {
                path.clone()
            };
            
            writeln!(writer, "\n### `{}`\n", relative_path.display())?;
            
            let language = get_file_extension(&path, config);
            
            match fs::read_to_string(&path) {
                Ok(content) => {
                    // Определяем необходимое количество бактиков
                    let fence_length = calculate_fence_length(&content);
                    let fence = "`".repeat(fence_length);
                    
                    writeln!(writer, "{}{}", fence, language)?;
                    
                    // Убедимся, что контент заканчивается переводом строки
                    let content = if content.ends_with('\n') {
                        content
                    } else {
                        format!("{}\n", content)
                    };
                    write!(writer, "{}", content)?;
                    
                    writeln!(writer, "{}", fence)?;
                }
                Err(_) => {
                    // Для файлов, которые не удалось прочитать, используем стандартные 3 бактика
                    writeln!(writer, "```")?;
                    writeln!(writer, "[Не удалось прочитать файл]")?;
                    writeln!(writer, "```")?;
                }
            }
        }
    }
    
    Ok(())
}

fn calculate_fence_length(content: &str) -> usize {
    let mut max_backticks = 0;
    let mut current_backticks = 0;
    
    // Проходим по всем символам контента
    for c in content.chars() {
        if c == '`' {
            current_backticks += 1;
        } else {
            if current_backticks > max_backticks {
                max_backticks = current_backticks;
            }
            current_backticks = 0;
        }
    }
    
    // Проверяем последовательность в конце строки
    if current_backticks > max_backticks {
        max_backticks = current_backticks;
    }
    
    // Используем минимум 3 бактика, но если в файле есть последовательность из 3 или более, то на 1 больше
    // Для особых случаев (Markdown, JavaScript) увеличиваем базовый минимум
    let base_minimum = if content.contains("```") || content.contains("`${") {
        4  // Для файлов, где вероятно есть блоки кода или template literals
    } else {
        3  // Для обычных файлов
    };
    
    std::cmp::max(base_minimum, max_backticks + 1)
}

fn should_skip_entry(
    path: &Path, 
    name: &str, 
    app_config: &AppConfig, 
    config: &Config,
    gitignore_matcher: &Option<Gitignore>,
) -> SkipReason {
    if let Some(matcher) = gitignore_matcher {
        if matcher.matched(path, path.is_dir()).is_ignore() {
            return if path.is_dir() {
                SkipReason::SkipWithEllipsis
            } else {
                SkipReason::Skip
            };
        }
    }
    
    if name.starts_with('.') && name != ".gitignore" {
        return SkipReason::Skip;
    }
    
    if path.is_dir() && config.exclude_dirs.iter().any(|dir| name == dir) {
        return SkipReason::SkipWithEllipsis;
    }
    
    if !path.is_dir() && config.exclude_files.iter().any(|pattern| {
        if pattern.starts_with("*.") {
            let ext = &pattern[2..];
            name.ends_with(ext) || name.contains(&format!(".{}", ext))
        } else {
            name == pattern
        }
    }) {
        return SkipReason::Skip;
    }
    
    if let Some(output_file) = &app_config.output_file {
        if let Some(output_name) = Path::new(output_file).file_name() {
            if name == output_name.to_string_lossy().as_ref() {
                return SkipReason::Skip;
            }
        }
    } else if name == "tree.md" {
        return SkipReason::Skip;
    }
    
    SkipReason::NoSkip
}

fn is_binary_file(path: &Path, config: &Config) -> bool {
    if let Some(ext) = path.extension() {
        let ext_str = ext.to_string_lossy().to_lowercase();
        config.exclude_extensions.iter().any(|e| e == &ext_str)
    } else {
        false
    }
}

fn is_file_too_large(path: &Path, config: &Config) -> bool {
    if let Some(max_size) = config.max_file_size {
        if let Ok(metadata) = fs::metadata(path) {
            return metadata.len() > max_size;
        }
    }
    false
}

fn get_file_extension(path: &Path, config: &Config) -> String {
    const DEFAULT_LANGUAGE: &str = "text";
    
    let Some(ext) = path.extension() else {
        return DEFAULT_LANGUAGE.to_string();
    };
    
    let ext_str = ext.to_string_lossy().to_lowercase();
    
    if let Some(mapping) = &config.extension_mapping {
        if let Some(language) = mapping.get(ext_str.as_str()) {
            return language.clone();
        }
    }
    
    DEFAULT_LANGUAGE.to_string()
}