# Splitter

A lightweight CLI tool that reads from stdin and splits the stream into multiple output files. Files are rotated either after a specified number of lines or after a configurable timeout — whichever comes first.

Useful for processing logs, streaming data, or long-running pipelines where output needs to be chunked into manageable files.

## Example Usage

```bash
some_streaming_command | splitter --lines 1000 --timeout 60
```

This creates a new file every 1000 lines or every 60 seconds, whichever happens first.
