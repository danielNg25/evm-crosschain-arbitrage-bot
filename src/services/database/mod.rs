// MongoDB modules
pub mod models;
pub mod mongodb;
// pub mod mongodb_logger;
pub mod repositories;
pub mod service;

// Re-export commonly used types
pub use mongodb::MongoDbClient;
pub use service::MongoDbService;

// Legacy sled Database (kept for backward compatibility if needed)
use anyhow::Result;
use log::{debug, info};
use sled::Db;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;

#[derive(Clone)]
pub struct Database {
    db: Arc<Db>,
}

impl Database {
    pub fn new<P: AsRef<Path>>(db_name: P) -> Result<Self> {
        // Create the data directory if it doesn't exist
        let data_dir = PathBuf::from("./data");
        std::fs::create_dir_all(&data_dir)?;

        // Get the database name as string
        let db_name_str = db_name.as_ref().to_string_lossy().to_string();

        // Create full path in ./data folder
        let db_path = data_dir.join(db_name_str);
        info!("Opening database at {:?}", db_path);

        let db = sled::open(db_path)?;
        info!("Database opened successfully");

        Ok(Self { db: Arc::new(db) })
    }

    pub fn get_tree(&self, name: &str) -> Result<sled::Tree> {
        let tree = self.db.open_tree(name)?;
        debug!("Opened tree: {}", name);
        Ok(tree)
    }

    pub fn serialize<T: serde::Serialize>(value: &T) -> Result<Vec<u8>> {
        Ok(bincode::serialize(value)?)
    }

    pub fn deserialize<T: serde::de::DeserializeOwned>(bytes: &[u8]) -> Result<T> {
        Ok(bincode::deserialize(bytes)?)
    }

    pub fn insert<K, V>(&self, tree_name: &str, key: K, value: &V) -> Result<()>
    where
        K: AsRef<[u8]>,
        V: serde::Serialize,
    {
        let tree = self.get_tree(tree_name)?;
        let serialized = Self::serialize(value)?;
        tree.insert(key, serialized)?;
        tree.flush()?;
        Ok(())
    }

    pub fn get<K, V>(&self, tree_name: &str, key: K) -> Result<Option<V>>
    where
        K: AsRef<[u8]>,
        V: serde::de::DeserializeOwned,
    {
        let tree = self.get_tree(tree_name)?;
        match tree.get(key)? {
            Some(bytes) => Ok(Some(Self::deserialize(&bytes)?)),
            None => Ok(None),
        }
    }

    pub fn remove<K>(&self, tree_name: &str, key: K) -> Result<()>
    where
        K: AsRef<[u8]>,
    {
        let tree = self.get_tree(tree_name)?;
        tree.remove(key)?;
        tree.flush()?;
        Ok(())
    }

    pub fn iter<V>(&self, tree_name: &str) -> Result<impl Iterator<Item = Result<(Vec<u8>, V)>>>
    where
        V: serde::de::DeserializeOwned,
    {
        let tree = self.get_tree(tree_name)?;
        let iter = tree.iter().map(|res| {
            res.map_err(|e| anyhow::anyhow!(e))
                .and_then(|(k, v)| Ok((k.to_vec(), Self::deserialize(&v)?)))
        });
        Ok(iter)
    }

    pub fn snapshot(&self) -> Result<()> {
        self.db.flush()?;
        Ok(())
    }
}
