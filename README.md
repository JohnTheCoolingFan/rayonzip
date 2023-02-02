# rayonzip

A library for creating zip files using rayon for thread control

This library is inspired by [mtzip](https://crates.io/crates/mtzip), which manages concurrency by itself.

After some tests, it's even slower than single-threaded program that uses [zip](https://crates.io/crates/zip) library. All tested on [rfmp](https://gitlab.com/JohnTheCoolingFan/rfmp).

Example usage:

```rs
use rayon::ThreadPoolBuilder;
use rayonzip::ZipArchive;

# Get amount of available threads for use
let threads = std::threads::available_parallelism.unwrap();

# Build a rayon thread pool
let thread_pool = ThreadPoolBuilder::new().num_thread(threads.into()).build().unwrap();

# Create a zp archive that'll use the thread pool to compress concurrently
let mut zipper = ZipArchive::new(&thread_pool);

# Add a file from filesystem
zipper.add_file_from_fs("input/test_text_file.txt", "test_text_file.txt");

# Add a file from binary slice
zipper.add_file_from_slice(b"Hello, world!", "hello_world.txt");

# Adding a directory and a file to it
zipper.add_directory("test_dir");
zipper.add_file("input/file_that_goes_to_a_dir.txt", "test_dir/file_that_goes_to_a_dir.txt");

# Writing to a file

# First, open/create a file
let mut file = File::create("output.zip").unwrap();
# Now, write the zip archive data to a file.
# This consumes the zip archive struct. Write to a buffer if you want to write to multiple destinations
zipper.write(&mut file).unwrap();
```
