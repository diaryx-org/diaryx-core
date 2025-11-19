use crate::fs::FileSystem;

pub struct DiaryxApp<FS: FileSystem> {
    fs: FS,
}

impl<FS: FileSystem> DiaryxApp<FS> {
    pub fn new(fs: FS) -> Self {
        Self { fs }
    }

    pub fn create_entry(&self, path: &str) -> std::io::Result<()> {
        let content = format!("---\ntitle: {}\n---\n\n# {}\n\n", path, path); 
        self.fs.create_new(std::path::Path::new(path), &content)?; 
        Ok(())
    }


}
