fn main() {
    // Ensure dashboard/dist exists so rust-embed doesn't fail.
    // If the SPA hasn't been built, serve a placeholder.
    let dist = std::path::Path::new("dashboard/dist");
    if !dist.join("index.html").exists() {
        std::fs::create_dir_all(dist).unwrap();
        std::fs::write(
            dist.join("index.html"),
            r#"<!DOCTYPE html>
<html><body style="font-family:monospace;padding:40px">
<h1>LAUDEC</h1>
<p>Dashboard not built. Run:</p>
<pre>cd dashboard && npm install && npm run build</pre>
</body></html>"#,
        )
        .unwrap();
    }
}
