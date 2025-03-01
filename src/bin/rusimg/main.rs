use std::path::{Path, PathBuf};
use std::fs;
use std::fmt;
use std::io::{stdout, Write};
use glob::glob;
use image::DynamicImage;
use parse::ArgStruct;
use colored::*;
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;
use futures::stream::FuturesUnordered;

use librusimg::{RusImg, RusimgError};
mod parse;

// Error types
type ErrorOccuredFilePath = String;
type ErrorMessage = std::io::Error;
/// Error structure containing the error and the file path where the error occurred.
struct ErrorStruct<T> {
    error: T,
    filepath: ErrorOccuredFilePath,
}
/// ProcessingError is an error type that occurs during image processing.
enum ProcessingError {
    RusimgError(ErrorStruct<RusimgError>),
    IOError(ErrorStruct<ErrorMessage>),
    FailedToViewImage(String),
}
impl fmt::Display for ProcessingError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            ProcessingError::RusimgError(e) => write!(f, "{}", e.error),
            ProcessingError::IOError(e) => write!(f, "{}", e.error),
            ProcessingError::FailedToViewImage(s) => write!(f, "Failed to view image: {}", s),
        }
    }
}

// result status
/// FileOverwriteAsk is an enum that represents the status of whether to overwrite a file.
/// This is used to determine whether to overwrite a file when it already exists.
/// - YesToAll: Overwrite all files without asking. This is used when the --yes option is specified.
/// - NoToAll: Skip all files without asking. This is used when the --no option is specified.
/// - AskEverytime: Ask every time.
#[derive(Debug, Clone, PartialEq)]
enum FileOverwriteAsk {
    YesToAll,
    NoToAll,
    AskEverytime,
}
/// ExistsCheckResult is an enum that represents the result of checking whether a file exists.
/// - AllOverwrite: Overwrite all files without asking. This is used when the --yes option is specified.
/// - AllSkip: Skip all files without asking. This is used when the --no option is specified.
/// - NeedToAsk: Ask every time.
/// - NoProblem: No problem. This means that the file does not exist.
enum ExistsCheckResult {
    AllOverwrite,
    AllSkip,
    NeedToAsk,
    NoProblem,
}
/// AskResult is an enum that represents the result of asking whether to overwrite a file.
/// - Overwrite: Overwrite the file.
/// - Skip: Skip the file.
/// - NoProblem: No problem. This means that the file does not exist.
enum AskResult {
    Overwrite,
    Skip,
    NoProblem,
}
/// RusimgStatus is an enum that represents the status of the image processing result.
/// - Success: The processing was successful.
/// - Cancel: The processing was canceled.
/// - NotNeeded: The processing was not needed. This is used when no processing is required.
#[derive(Debug, Clone, PartialEq)]
enum RusimgStatus {
    Success,
    Cancel,
    NotNeeded,
}

/// ThreadTask is a structure that represents the task to be executed by each thread.
/// - args: Arguments passed to the program.
/// - input_path: The path to the input image file.
/// - output_path: The path to the output image file.
/// - extension: The extension of the output image file.
/// - ask_result: The result of asking whether to overwrite the file.
struct ThreadTask {
    args: ArgStruct,
    input_path: PathBuf,
    output_path: Option<PathBuf>,
    extension: Option<librusimg::Extension>,
    ask_result: AskResult,
}

/// ConvertResult is a structure that represents the result of converting an image.
/// This structure will be used to display the result of the conversion.
/// - before_extension: The extension of the image before conversion.
/// - after_extension: The extension of the image after conversion.
struct ConvertResult {
    before_extension: librusimg::Extension,
    after_extension: librusimg::Extension,
}
/// TrimResult is a structure that represents the result of trimming an image.
/// This structure will be used to display the result of the trimming.
/// - before_size: The size of the image before trimming.
/// - after_size: The size of the image after trimming.
struct TrimResult {
    before_size: librusimg::ImgSize,
    after_size: librusimg::ImgSize,
}
/// ResizeResult is a structure that represents the result of resizing an image.
/// This structure will be used to display the result of the resizing.
/// - before_size: The size of the image before resizing.
/// - after_size: The size of the image after resizing.
struct ResizeResult {
    before_size: librusimg::ImgSize,
    after_size: librusimg::ImgSize,
}
/// GrayscaleResult is a structure that represents the result of converting an image to grayscale.
/// This structure will be used to display the result of the grayscale conversion.
/// - status: The status of the grayscale conversion.
struct GrayscaleResult {
    status: bool,
}
/// CompressResult is a structure that represents the result of compressing an image.
/// This structure will be used to display the result of the compression.
/// - status: The status of the compression.
struct CompressResult {
    status: bool,
}
/// SaveResult is a structure that represents the result of saving an image.
/// This structure will be used to display the result of the saving.
/// - status: The status of the saving.
/// - input_path: The path to the input image file.
/// - output_path: The path to the output image file.
/// - before_filesize: The size of the image before saving.
/// - after_filesize: The size of the image after saving. If the image was not saved, this value will be None.
/// - delete: Whether to delete the original file.
struct SaveResult {
    status: RusimgStatus,
    input_path: PathBuf,
    output_path: Option<PathBuf>,
    before_filesize: u64,
    after_filesize: Option<u64>,
    delete: bool,
}
/// ProcessResult is a structure that represents the result of processing an image.
/// This structure contains the results of each processing step.
struct ProcessResult {
    viuer_image: Option<DynamicImage>,
    convert_result: Option<ConvertResult>,
    trim_result: Option<TrimResult>,
    resize_result: Option<ResizeResult>,
    grayscale_result: Option<GrayscaleResult>,
    compress_result: Option<CompressResult>,
    save_result: SaveResult,
}
/// ThreadResult is a structure that represents the result of processing an image in a thread.
/// This structure contains the processing result and a flag indicating whether the processing is complete.
struct ThreadResult {
    process_result: Option<Result<ProcessResult, ProcessingError>>,
    finish: bool,
}

/// Get the list of files in the directory.
/// This function used to get the list of image files in the directory when the --source option is specified with a directory path.
/// - dir_path: The path to the directory.
/// - recursive: Whether to search recursively.
fn get_files_in_dir(dir_path: &PathBuf, recursive: bool) -> Result<Vec<PathBuf>, String> {
    let mut files = fs::read_dir(&dir_path).expect("cannot read directory");
    let mut ret = Vec::new();

    while let Some(file) = files.next() {
        let dir_entry = file;
        match dir_entry {
            Ok(dir_entry) => {
                let path = dir_entry.path();
                // recursive に探索
                if path.is_dir() && recursive {
                    let mut files = get_files_in_dir(&path, recursive)?;
                    ret.append(&mut files);
                }
                else {
                    let file_name = dir_entry.file_name().into_string().expect("cannot convert file name");
                    if get_extension(&Path::new(&file_name)).is_ok() {
                        ret.push(Path::new(&dir_path).join(&file_name));
                    }
                }
            },
            Err(e) => {
                println!("cannot read a directory entry: {}", e.to_string());
                continue;
            },
        }
    }
    Ok(ret)
}

/// Get the list of files by wildcard.
/// This function used to get the list of image files by wildcard when the --source option is specified with a wildcard pattern.
fn get_files_by_wildcard(source_path: &PathBuf) -> Result<Vec<PathBuf>, String> {
    let mut ret = Vec::new();
    for entry in glob(source_path.to_str().unwrap()).expect("Failed to read glob pattern") {
        match entry {
            Ok(path) => {
                // 画像形式であればファイルリストに追加
                if get_extension(&path).is_ok() {
                    ret.push(path);
                }
            },
            Err(e) => println!("{:?}", e),
        }
    }
    Ok(ret)
}

/// Convert a string to an image extension.
fn convert_str_to_extension(extension_str: &str) -> Result<librusimg::Extension, RusimgError> {
    match extension_str {
        "bmp" => Ok(librusimg::Extension::Bmp),
        "jpg" | "jpeg" | "jfif" => Ok(librusimg::Extension::Jpeg),
        "png" => Ok(librusimg::Extension::Png),
        "webp" => Ok(librusimg::Extension::Webp),
        _ => Err(RusimgError::UnsupportedFileExtension),
    }
}

/// Get the extension of the file.
fn get_extension(path: &Path) -> Result<librusimg::Extension, RusimgError> {
    let path = path.to_str().ok_or(RusimgError::FailedToConvertPathToString)?.to_ascii_lowercase();
    match Path::new(&path).extension().and_then(|s| s.to_str()) {
        Some("bmp") => Ok(librusimg::Extension::Bmp),
        Some("jpg") | Some("jpeg") | Some("jfif") => Ok(librusimg::Extension::Jpeg),
        Some("png") => Ok(librusimg::Extension::Png),
        Some("webp") => Ok(librusimg::Extension::Webp),
        _ => {
            Err(RusimgError::UnsupportedFileExtension)
        },
    }
}

/// Determine the output path.
fn get_output_path(args: &ArgStruct, input_path: &PathBuf, extension: &librusimg::Extension) -> PathBuf {
    let extension = if args.double_extension {
        format!("{}.{}", input_path.extension().unwrap().to_str().unwrap(), extension.to_string())
    }
    else {
        extension.to_string()
    };
    let mut output_path = match &args.destination_path {
        Some(path) => path.clone(),                                                             // If --output is specified, use it
        None => Path::new(input_path).with_extension(&extension),       // If not, use the input filepath as the input file
    };
    // If append_name is specified, add it to the file name.
    if let Some(append_name) = &args.destination_append_name {
        let mut output_path_tmp = output_path.file_stem().unwrap().to_str().unwrap().to_string();
        output_path_tmp.push_str(append_name);
        output_path_tmp.push_str(".");
        output_path_tmp.push_str(&extension);
        output_path = PathBuf::from(output_path_tmp);
    }
    output_path
}

/// Check if the file exists.
/// If the file exists, check if it should be overwritten.
fn check_file_exists(path: &PathBuf, file_overwrite_ask: &FileOverwriteAsk) -> ExistsCheckResult {
    if Path::new(path).exists() {
        println!("The image file \"{}\" already exists.", path.display().to_string().yellow().bold());
        match file_overwrite_ask {
            FileOverwriteAsk::YesToAll => {
                return ExistsCheckResult::AllOverwrite;
            },
            FileOverwriteAsk::NoToAll => {
                return ExistsCheckResult::AllSkip;
            },
            FileOverwriteAsk::AskEverytime => {
                return ExistsCheckResult::NeedToAsk;
            },
        }
    }
    return ExistsCheckResult::NoProblem;
}

/// Ask if the file should be overwritten.
fn ask_file_exists() -> bool {
    print!(" Do you want to overwrite it? [y/N]: ");
    loop {
        stdout().flush().unwrap();

        let mut input = String::new();
        std::io::stdin().read_line(&mut input).unwrap();
        if input.trim().to_ascii_lowercase() == "y" || input.trim().to_ascii_lowercase() == "yes" {
            println!(" => The file will be overwritten.");
            return true;
        }
        else if input.trim().to_ascii_lowercase() == "n" || input.trim().to_ascii_lowercase() == "no" || input.trim() == "" {
            println!(" => The file will be skipped.");
            return false;
        }
        else {
            print!(" Please enter y or n [y/N]: ");
        }
    }
}

/// Show the result of saving the image.
fn save_print(before_path: &PathBuf, after_path: &Option<PathBuf>, before_size: u64, after_size: Option<u64>) {
    match (after_path, after_size) {
        (Some(after_path), Some(after_size)) => {
            if before_path == after_path {
                println!("{}: {}", "Overwrite", before_path.display());
                println!("File Size: {} -> {} ({:.1}%)", before_size, after_size, (after_size as f64 / before_size as f64) * 100.0);
            }
            else if get_extension(before_path.as_path()) != get_extension(after_path.as_path()) {
                println!("{}: {} -> {}", "Rename", before_path.display(), after_path.display());
                println!("File Size: {} -> {} ({:.1}%)", before_size, after_size, (after_size as f64 / before_size as f64) * 100.0);
            }
            else {
                println!("{}: {} -> {}", "Move", before_path.display(), after_path.display());
                println!("File Size: {} -> {} ({:.1}%)", before_size, after_size, (after_size as f64 / before_size as f64) * 100.0);
            }
        },
        (_, _) => {
            return;
        },
    }
}

/// Show the image in the terminal using viuer.
/// Read the image data from memory and display it.
fn view(image: &DynamicImage) -> Result<(), ProcessingError> {
    let width = image.width();
    let height = image.height();
    let conf_width = width as f64 / std::cmp::max(width, height) as f64 * 100 as f64;
    let conf_height = height as f64 / std::cmp::max(width, height) as f64 as f64 * 50 as f64;
    let conf = viuer::Config {
        absolute_offset: false,
        width: Some(conf_width as u32),
        height: Some(conf_height as u32),    
        ..Default::default()
    };
    
    let result = viuer::print(&image, &conf);
    match result {
        Ok(_) => Ok(()),
        Err(e) => Err(ProcessingError::FailedToViewImage(e.to_string())),
    }
}

/// Convert an image.
fn process_convert<C: Fn(RusimgError) -> ProcessingError>(extension: &Option<librusimg::Extension>, image: &mut RusImg, rierr: C) -> Result<Option<ConvertResult>, ProcessingError> {
    if let Some(extension) = extension {
        let before_extension = image.extension.clone();

        // 変換
        image.convert(&extension).map_err(rierr)?;

        Ok(Some(ConvertResult {
            before_extension: before_extension,
            after_extension: extension.clone(),
        }))
    }
    else {
        Err(rierr(RusimgError::FailedToConvertExtension))
    }
}

/// Trim an image.
fn process_trim<C: Fn(RusimgError) -> ProcessingError>(image: &mut RusImg, trim: librusimg::Rect, rierr: C) -> Result<Option<TrimResult>, ProcessingError> {
    // トリミング
    let before_size = image.get_image_size().map_err(&rierr)?;
    let after_size = image.trim_rect(trim).map_err(&rierr)?;

    Ok(Some(TrimResult {
        before_size: before_size,
        after_size: after_size,
    }))
}

/// Process the image in a thread.
async fn process(thread_task: ThreadTask, file_io_lock: Arc<Mutex<i32>>) -> Result<ProcessResult, ProcessingError> {
    let args = thread_task.args;
    let image_file_path = thread_task.input_path;
    let output_file_path = thread_task.output_path;
    let ask_result = thread_task.ask_result;

    let rierr = |e: RusimgError| ProcessingError::RusimgError(ErrorStruct { error: e, filepath: image_file_path.to_str().unwrap().to_string() });
    let ioerr = |e: std::io::Error| ProcessingError::IOError(ErrorStruct { error: e, filepath: image_file_path.to_str().unwrap().to_string() });

    // Open the image
    let mut image = librusimg::open_image(&image_file_path).map_err(rierr)?;

    // Is saving the image required? (default: false)
    let mut save_required = false;

    // --convert -> Convert the image.
    let convert_result = if let Some(_c) = args.destination_extension {
        save_required = true;
        process_convert(&thread_task.extension, &mut image, rierr)?
    }
    else {
        None
    };

    // --trim -> Trim the image.
    let trim_result = if let Some(trim) = args.trim {
        save_required = true;
        process_trim(&mut image, trim, rierr)?
    }
    else {
        None
    };

    // --resize -> Resize the image.
    let resize_result = if let Some(resize) = args.resize {
        let before_size = image.get_image_size().map_err(rierr)?;
        let after_size = image.resize(resize).map_err(rierr)?;
        save_required = true;

        Some(ResizeResult {
            before_size: before_size,
            after_size: after_size,
        })
    }
    else {
        None
    };

    // --grayscale -> Convert the image to grayscale.
    let grayscale_result = if args.grayscale {
        image.grayscale().map_err(rierr)?;
        save_required = true;

        Some(GrayscaleResult {
            status: true,
        })
    }
    else {
        None
    };

    // --quality -> Compress the image.
    let compress_result = if let Some(q) = args.quality {
        image.compress(Some(q)).map_err(rierr)?;
        save_required = true;

        Some(CompressResult {
            status: true,
        })
    }
    else {
        None
    };

    // --view -> View the image in the terminal.
    // Viuer will be called after all processing is complete.
    // So, store the image data in memory.
    let viuer_image = if args.view {
        Some(image.get_dynamic_image().map_err(rierr)?)
    }
    else {
        None
    };

    // Save the image if necessary.
    let save_status = if save_required == true {
        // Check if the file exists and ask if it should be overwritten.
        match ask_result {
            AskResult::Overwrite => {
                // If AskResult::Overwrite, overwrite the file without asking.
            },
            AskResult::Skip => {
                // If AskResult::Skip, skip the file.
                return Ok(ProcessResult {
                    viuer_image: viuer_image,
                    convert_result: convert_result,
                    trim_result: trim_result,
                    resize_result: resize_result,
                    grayscale_result: grayscale_result,
                    compress_result: compress_result,
                    save_result: SaveResult {
                        status: RusimgStatus::Cancel,
                        input_path: image.get_input_filepath(),
                        output_path: None,
                        before_filesize: 0,
                        after_filesize: None,
                        delete: false,
                    },
                });
            },
            AskResult::NoProblem => {
                // If no problem, save the file.
            },
        }

        // Get the output path
        let output_path = output_file_path.unwrap();

        // Save the image
        // Saving images at the same time can be a heavy load, so we need to lock the file I/O.
        // *lock is used to lock the file I/O.
        let save_status = {
            let mut lock = file_io_lock.lock().unwrap();
            *lock += 1;
            let ret = image.save_image(output_path.to_str()).map_err(rierr)?;
            ret
        };

        // --delete -> Delete the original file. 
        let delete = if let Some(saved_filepath) = save_status.output_path.clone() {
            if args.delete && image_file_path != saved_filepath {
                fs::remove_file(&image_file_path).map_err(ioerr)?;
                true
            }
            else {
                false
            }
        }
        else {
            false
        };

        // Return the result of saving the image.
        SaveResult {
            status: RusimgStatus::Success,
            input_path: image.get_input_filepath(),
            output_path: save_status.output_path,
            before_filesize: save_status.before_filesize,
            after_filesize: save_status.after_filesize,
            delete: delete,
        }
    }
    else {
        // If saving is not required, return the status as NotNeeded.
        SaveResult {
            status: RusimgStatus::NotNeeded,
            input_path: image.get_input_filepath(),
            output_path: None,
            before_filesize: 0,
            after_filesize: None,
            delete: false,
        }
    };

    // Return the processing result.
    let thread_results = ProcessResult {
        viuer_image: viuer_image,
        convert_result: convert_result,
        trim_result: trim_result,
        resize_result: resize_result,
        grayscale_result: grayscale_result,
        compress_result: compress_result,
        save_result: save_status,
    };
    Ok(thread_results)
}

#[tokio::main]
async fn main() -> Result<(), String> {
    // Parse the arguments.
    let args = parse::parser().map_err(|e| e.to_string())?;

    // Number of threads.
    let threads = args.threads;

    // Is it necessary to confirm every time if overwriting is required?
    // -y, --yes: Always overwrite
    // -n, --no: Always skip
    // If neither is specified, ask every time.
    let file_overwrite_ask = if args.yes {
        FileOverwriteAsk::YesToAll
    }
    else if args.no {
        FileOverwriteAsk::NoToAll
    }
    else {
        FileOverwriteAsk::AskEverytime
    };

    // Specify the source path.
    // Default: current directory
    let source_paths = args.souce_path.clone().or(Some(vec![PathBuf::from(".")])).unwrap();
    let mut thread_tasks = Vec::new();
    for source_path in source_paths {
        let image_files_list = if source_path.is_dir() {
            get_files_in_dir(&source_path, args.recursive)?
        }
        else {
            get_files_by_wildcard(&source_path)?
        };
        for image_file in image_files_list {
            let thread_task = if let Some(extension_str) = &args.destination_extension {
                // Determine the output path.
                let extension = convert_str_to_extension(&extension_str.clone());
                let extension = match extension {
                    Ok(e) => e,
                    Err(e) => {
                        println!("{}: {}", "Error".red(), e.to_string());
                        continue;
                    },
                };
                let output_path = get_output_path(&args, &image_file, &extension);

                // If the output file already exists, check if it should be overwritten.
                let ask_result = match check_file_exists(&output_path, &file_overwrite_ask) {
                    // Print the result of checking if the file exists.
                    ExistsCheckResult::AllOverwrite => {
                        println!("{}", " => Overwrite (default: yes)".bold());
                        AskResult::Overwrite
                    },
                    ExistsCheckResult::AllSkip => {
                        println!("{}", " => Skip (default: no)".bold());
                        AskResult::Skip
                    },
                    ExistsCheckResult::NeedToAsk => {
                        // If the file exists, ask if it should be overwritten.
                        if ask_file_exists() {
                            AskResult::Overwrite
                        }
                        else {
                            AskResult::Skip
                        }
                    },
                    ExistsCheckResult::NoProblem => {
                        AskResult::NoProblem
                    },
                };

                // Make a thread task.
                ThreadTask {
                    args: args.clone(),
                    input_path: image_file,
                    output_path: Some(output_path),
                    extension: Some(extension),
                    ask_result: ask_result,
                }
            }
            else {
                // If saving is not required, create a thread task without an output path.
                ThreadTask {
                    args: args.clone(),
                    input_path: image_file,
                    output_path: None,
                    extension: None,
                    ask_result: AskResult::NoProblem,
                }
            };
            
            // Add the thread task to the thread_tasks.
            thread_tasks.push(thread_task);
        }
    }

    // Display the number of images detected.
    let total_image_count = thread_tasks.len();
    println!("{}", format!("🔎 {} images are detected.", total_image_count).bold());

    // Share thread_tasks between threads.
    let thread_tasks = Arc::new(Mutex::new(thread_tasks));

    // Processing for each image..
    let mut error_count = 0;
    let count = Arc::new(Mutex::new(0));
    let tasks = FuturesUnordered::new();
    
    // Prepare a channel to communicate between threads.
    let (tx, mut rx) = mpsc::channel::<ThreadResult>(32);

    // Lock for file I/O
    let file_io_lock = Arc::new(Mutex::new(0));

    // Start processing in each thread.
    for _thread_num in 0..threads {
        let thread_tasks = Arc::clone(&thread_tasks);
        let count = Arc::clone(&count);
        let tx = tx.clone();
        let file_io_lock = Arc::clone(&file_io_lock);
        
        let thread = tokio::spawn(async move {
            loop {
                let thread_task = {
                    let mut thread_tasks = thread_tasks.lock().unwrap();
                    thread_tasks.pop()
                };
                if thread_task.is_none() {
                    match tx.send(ThreadResult {
                        process_result: None,
                        finish: true,
                    }).await {
                        Ok(_) => {},
                        Err(e) => {
                            println!("Send error: {}", e.to_string());
                        }
                    }
                    break;
                }
                let thread_task = thread_task.unwrap();
                /*
                let processing_str = format!("[{}/{}] Processing: {}", count, total_image_count, &Path::new(&thread_task.input_path).file_name().unwrap().to_str().unwrap());
                println!("{}", processing_str.yellow().bold());
                */
                let process_result = process(thread_task, file_io_lock.clone()).await;
                match tx.send(ThreadResult {
                    process_result: Some(process_result),
                    finish: false,
                }).await {
                    Ok(_) => {},
                    Err(e) => {
                        println!("Send error: {}", e.to_string());
                    }
                }

                // Count up the number of processed images.
                let mut count = count.lock().unwrap();
                *count += 1;
            }
        });
        tasks.push(thread);
    }

    // Display the results of the threads.
    let mut count = 0;
    let mut thread_finished = 0;
    while let Some(rx_result) = rx.recv().await {
        if let Some(process_result) = rx_result.process_result {
            match process_result {
                // If the processing is successful, display the result.
                Ok(thread_results) => {
                    count = count + 1;
                    let processing_str = format!("[{}/{}] Finish: {}", count + error_count, total_image_count, &Path::new(&thread_results.save_result.input_path).file_name().unwrap().to_str().unwrap());
                    println!("{}", processing_str.yellow().bold());

                    if let Some(convert_result) = thread_results.convert_result {
                        println!("Convert: {} -> {}", convert_result.before_extension.to_string(), convert_result.after_extension.to_string());
                    }
                    if let Some(trim_result) = thread_results.trim_result {
                        println!("Trim: {}x{} -> {}x{}", trim_result.before_size.width, trim_result.before_size.height, trim_result.after_size.width, trim_result.after_size.height);
                    }
                    if let Some(resize_result) = thread_results.resize_result {
                        println!("Resize: {}x{} -> {}x{}", resize_result.before_size.width, resize_result.before_size.height, resize_result.after_size.width, resize_result.after_size.height);
                    }
                    if let Some(grayscale_result) = thread_results.grayscale_result {
                        if grayscale_result.status {
                            println!("Grayscale: Done.");
                        }
                    }
                    if let Some(compress_result) = thread_results.compress_result {
                        if compress_result.status {
                            println!("Compress: Done.");
                        }
                    }

                    // Show the image in the terminal.
                    // Use viuer crate to display the image.
                    if let Some(viuer_image) = thread_results.viuer_image {
                        view(&viuer_image).map_err(|e| e.to_string()).unwrap();
                    }

                    match thread_results.save_result.status {
                        RusimgStatus::Success => {
                            // Print the result of saving the image.
                            save_print(&thread_results.save_result.input_path, &thread_results.save_result.output_path,
                                thread_results.save_result.before_filesize, thread_results.save_result.after_filesize);

                            if thread_results.save_result.delete {
                                println!("Delete source file: {}", thread_results.save_result.input_path.display());
                            }
                            println!("{}", "Success.".green().bold())
                        },
                        RusimgStatus::Cancel => println!("{}", "Canceled.".yellow().bold()),
                        RusimgStatus::NotNeeded => println!("{}", "Nothing to do.".yellow().bold()),
                    };
                }
                // If an error occurs during processing, display the error.
                Err(e) => {
                    error_count = error_count + 1;
                    match e {
                        ProcessingError::RusimgError(e) => {
                            let processing_str = format!("[{}/{}] Failed: {}", count + error_count, total_image_count, &Path::new(&e.filepath).file_name().unwrap().to_str().unwrap());
                            println!("{}", processing_str.red().bold());
                            println!("{}: {}", "Error".red(), e.error);
                        },
                        ProcessingError::IOError(e) => {
                            let processing_str = format!("[{}/{}] Failed: {}", count + error_count, total_image_count, &Path::new(&e.filepath).file_name().unwrap().to_str().unwrap());
                            println!("{}", processing_str.red().bold());
                            println!("{}: {}", "Error".red(), e.error);
                        },
                        ProcessingError::FailedToViewImage(s) => {
                            println!("{}: {}", "Error".red(), s);
                        },
                    }
                }
            }
        }

        if rx_result.finish {
            thread_finished = thread_finished + 1;
        }
        // If all threads are finished, break the loop.
        if thread_finished == threads {
            break;
        }
    }

    // Show the result of processing all images.
    if error_count > 0 {
        println!("\n✅ {} images are processed.", total_image_count - error_count);
        println!("❌ {} images are failed to process.", error_count);
    }
    else {
        println!("\n✅ All images are processed.");
    }

    Ok(())
}
