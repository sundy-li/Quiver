// Licensed to the Apache Software Foundation (ASF) under one
// or more contributor license agreements.  See the NOTICE file
// distributed with this work for additional information
// regarding copyright ownership.  The ASF licenses this file
// to you under the Apache License, Version 2.0 (the
// "License"); you may not use this file except in compliance
// with the License.  You may obtain a copy of the License at
//
//   http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing,
// software distributed under the License is distributed on an
// "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied.  See the License for the
// specific language governing permissions and limitations
// under the License.

use std::fs::File;

use arrow::array::Array;
use arrow::chunk::Chunk;
use arrow::datatypes::Schema;
use arrow::error::Result;
use std::io::Write;
use strawboat::{write, CommonCompression};

fn write_batches(path: &str, schema: Schema, chunks: &[Chunk<Box<dyn Array>>]) -> Result<()> {
    let file = File::create(path)?;

    let options = write::WriteOptions {
        default_compression: CommonCompression::LZ4,
        default_compress_ratio: None,
        max_page_size: Some(8192),
        forbidden_compressions: vec![],
    };
    let mut writer = write::NativeWriter::new(file, schema, options);

    writer.start()?;
    for chunk in chunks {
        writer.write(chunk)?
    }

    writer.finish()?;

    let metas = serde_json::to_vec(&writer.metas).unwrap();
    let mut meta_file = File::options()
        .create(true)
        .write(true)
        .truncate(true)
        .open("/tmp/pa.st")?;
    meta_file.write_all(&metas)?;
    meta_file.flush()?;
    Ok(())
}

// cargo run --example strawboat_write --release /tmp/input.str
fn main() -> Result<()> {
    use std::env;
    let args: Vec<String> = env::args().collect();

    let file_path = &args[1];
    let (chunk, schema) = read_chunk();
    // write it
    write_batches(file_path, schema, &[chunk])?;

    Ok(())
}

fn read_chunk() -> (Chunk<Box<dyn Array>>, Schema) {
    let file_path = "/tmp/input.parquet";
    let mut reader = File::open(file_path).unwrap();

    // we can read its metadata:
    let metadata = arrow::io::parquet::read::read_metadata(&mut reader).unwrap();
    // and infer a [`Schema`] from the `metadata`.
    let schema = arrow::io::parquet::read::infer_schema(&metadata).unwrap();
    // we can filter the columns we need (here we select all)
    let schema = schema.filter(|_index, _field| true);

    // we can read the statistics of all parquet's row groups (here for each field)
    for field in &schema.fields {
        let statistics =
            arrow::io::parquet::read::statistics::deserialize(field, &metadata.row_groups).unwrap();
        println!("{statistics:#?}");
    }

    // say we found that we only need to read the first two row groups, "0" and "1"
    let row_groups = metadata
        .row_groups
        .into_iter()
        .enumerate()
        .filter(|(index, _)| *index == 0 || *index == 1)
        .map(|(_, row_group)| row_group)
        .collect();

    // we can then read the row groups into chunks
    let mut chunks = arrow::io::parquet::read::FileReader::new(
        reader,
        row_groups,
        schema.clone(),
        Some(usize::MAX),
        None,
        None,
    );

    if let Some(maybe_chunk) = chunks.next() {
        let chunk = maybe_chunk.unwrap();
        println!("chunk len -> {:?}", chunk.len());
        return (chunk, schema);
    }
    unreachable!()
}
