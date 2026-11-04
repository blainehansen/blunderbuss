```sh
cargo install --path . --force
```


```rust
let mut file = tokio::fs:;File::options()
  .append(true)
  .create(true)
  .open(filename)
  .await?;

file.write_all(b"Appending this line to the file\n").await?;
```
