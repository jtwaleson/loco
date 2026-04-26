use std::path::{Path, PathBuf};

use super::tera_builtins;
use crate::{controller::views::ViewRenderer, Error, Result};
use serde::Serialize;

pub static DEFAULT_ASSET_FOLDER: &str = "assets";

#[derive(Clone)]
pub struct TeraView(std::sync::Arc<tera::Tera>);

impl TeraView {
    /// Create a Tera view engine
    ///
    /// # Errors
    ///
    /// This function will return an error if building fails
    pub fn build() -> Result<Self> {
        Self::from_custom_dir(&PathBuf::from(DEFAULT_ASSET_FOLDER).join("views"), |_| {
            Ok(())
        })
    }

    /// Create a Tera view engine with a post-processing function for subsequent instantiation.
    ///
    /// The post-processing function is also run during the call to this method.
    ///
    /// # Errors
    ///
    /// This function will return an error if building fails or if the post-processing function fails
    pub fn build_with_post_process(
        post_process: impl Fn(&mut tera::Tera) -> Result<()> + Send + Sync + 'static,
    ) -> Result<Self> {
        Self::from_custom_dir(
            &PathBuf::from(DEFAULT_ASSET_FOLDER).join("views"),
            post_process,
        )
    }

    /// Create a new Tera instance from a directory path
    ///
    /// # Errors
    ///
    /// This function will return an error if building fails
    fn create_tera_instance<P: AsRef<Path>>(path: P) -> Result<tera::Tera> {
        let path = path
            .as_ref()
            .to_str()
            .ok_or_else(|| Error::string("invalid glob"))?;

        let mut tera = tera::Tera::new(path)?;

        tera_builtins::filters::register_filters(&mut tera);

        Ok(tera)
    }

    /// Create a Tera view engine from a custom directory
    ///
    /// The post-processing function is also run during the call to this method.
    ///
    /// # Errors
    ///
    /// This function will return an error if building fails or if the post-processing function fails
    pub fn from_custom_dir<P: AsRef<Path>>(
        path: &P,
        post_process: impl Fn(&mut tera::Tera) -> Result<()> + Send + Sync + 'static,
    ) -> Result<Self> {
        if !path.as_ref().exists() {
            return Err(Error::string(&format!(
                "missing views directory: `{}`",
                path.as_ref().display()
            )));
        }
        let view_dir = path.as_ref();
        let view_path: PathBuf = view_dir.join("**").join("*.html");

        // Create instance
        let mut tera = Self::create_tera_instance(&view_path)?;

        // Do post processing
        post_process(&mut tera)?;

        let tera = std::sync::Arc::new(tera);

        Ok(Self(tera))
    }
}

impl ViewRenderer for TeraView {
    fn render<S: Serialize>(&self, key: &str, data: S) -> Result<String> {
        let context = tera::Context::from_serialize(data)?;
        Ok(self.0.render(key, &context)?)
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use tree_fs;

    use super::*;
    #[test]
    fn can_render_view() {
        let tree_fs = tree_fs::TreeBuilder::default()
            .add_file("template/test.html", "generate test.html file: {{foo}}")
            .add_file("template/test2.html", "generate test2.html file: {{bar}}")
            .create()
            .unwrap();

        let v = TeraView::from_custom_dir(&tree_fs.root, |_| Ok(())).unwrap();

        assert_eq!(
            v.render("template/test.html", json!({"foo": "foo-txt"}))
                .unwrap(),
            "generate test.html file: foo-txt"
        );

        assert_eq!(
            v.render("template/test2.html", json!({"bar": "bar-txt"}))
                .unwrap(),
            "generate test2.html file: bar-txt"
        );
    }

}
