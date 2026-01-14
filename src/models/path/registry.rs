use alloy::primitives::Address;
use anyhow::Result;
use log::info;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

pub type PoolPath = Vec<PoolDirection>;
#[derive(Debug, Copy, Clone, Serialize, Deserialize, Hash, Eq, PartialEq)]
pub struct PoolDirection {
    pub pool: Address,
    pub token_in: Address,
    pub token_out: Address,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SingleChainPathsWithAnchorToken {
    pub paths: Vec<PoolPath>, // many paths to the same chain
    pub chain_id: u64,
    pub anchor_token: Address,
}

// Full path is a collection of one side paths with anchor token
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FullPath {
    pub paths: Vec<SingleChainPathsWithAnchorToken>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SingleFullPath {
    pub source_pool_path: SingleChainPathsWithAnchorToken,
    pub target_pool_paths: Vec<SingleChainPathsWithAnchorToken>,
}

impl PoolDirection {
    pub fn to_string(&self) -> String {
        let pool_str = format!("{:?}", self.pool);
        let token_in_str = format!("{:?}", self.token_in);
        let token_out_str = format!("{:?}", self.token_out);

        format!(
            "Pool: {}...{} | Token In: {}...{} → Token Out: {}...{}",
            &pool_str[0..6],
            &pool_str[pool_str.len() - 4..],
            &token_in_str[0..6],
            &token_in_str[token_in_str.len() - 4..],
            &token_out_str[0..6],
            &token_out_str[token_out_str.len() - 4..]
        )
    }
}
pub struct MultichainPathRegistry {
    pub path_registry: Arc<RwLock<HashMap<u64, Arc<RwLock<PathRegistry>>>>>,
}

impl MultichainPathRegistry {
    pub fn new() -> Self {
        Self {
            path_registry: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub async fn new_path_registry(&mut self, chain_id: u64) -> Result<()> {
        self.path_registry
            .write()
            .await
            .entry(chain_id)
            .or_insert_with(|| Arc::new(RwLock::new(PathRegistry::new(chain_id))));
        Ok(())
    }

    pub async fn get_path_registry(&self, chain_id: u64) -> Option<Arc<RwLock<PathRegistry>>> {
        self.path_registry
            .read()
            .await
            .get(&chain_id)
            .map(Arc::clone)
    }

    pub async fn remove_path_registry(&mut self, chain_id: u64) -> Result<()> {
        self.path_registry.write().await.remove(&chain_id);
        Ok(())
    }

    pub async fn set_paths(&mut self, paths: Vec<SingleChainPathsWithAnchorToken>) -> Result<()> {
        for path in &paths {
            // verify path is valid
            let anchor_token = path.anchor_token;
            let mut pool_to_path = HashMap::new();
            for single_path in &path.paths {
                if single_path.first().unwrap().token_in != anchor_token {
                    return Err(anyhow::anyhow!(
                        "First token in path is not the anchor token"
                    ));
                }

                let len = single_path.len();
                if len >= 2 {
                    for i in 0..len - 2 {
                        if single_path[i].token_out != single_path[i + 1].token_in {
                            return Err(anyhow::anyhow!("Path is not valid"));
                        }
                    }
                }

                let pool = single_path[0].pool;
                if !pool_to_path.contains_key(&pool) {
                    pool_to_path.insert(pool, Vec::new());
                }
                pool_to_path
                    .get_mut(&pool)
                    .unwrap()
                    .push(single_path.clone());
            }
            let path_registry = self.get_path_registry(path.chain_id).await.unwrap();
            // Filter out paths for the current chain_id - only include paths for other chains
            let other_paths: Vec<SingleChainPathsWithAnchorToken> = paths
                .iter()
                .filter(|p| p.chain_id != path.chain_id)
                .cloned()
                .collect();

            for (pool, paths) in pool_to_path {
                path_registry
                    .write()
                    .await
                    .set_paths(
                        pool,
                        SingleChainPathsWithAnchorToken {
                            paths: paths,
                            chain_id: path.chain_id,
                            anchor_token: path.anchor_token,
                        },
                        other_paths.clone(),
                    )
                    .await?;
            }
        }

        Ok(())
    }

    pub async fn get_paths_for_pool(
        &self,
        chain_id: u64,
        pool: Address,
    ) -> Option<(
        SingleChainPathsWithAnchorToken,
        Vec<SingleChainPathsWithAnchorToken>,
    )> {
        let path_registry = self.get_path_registry(chain_id).await.unwrap();
        let guard = path_registry.read().await;
        guard.get_paths_for_pool(pool).await
    }
}

#[derive(Default)]
pub struct PathRegistry {
    pub chain_id: u64,
    // Cache of paths found for pool => other chain
    paths_cache: Arc<
        RwLock<
            HashMap<
                Address,
                (
                    // source_chain
                    SingleChainPathsWithAnchorToken,
                    // target_chain
                    Vec<SingleChainPathsWithAnchorToken>,
                ),
            >,
        >,
    >,
}

impl PathRegistry {
    pub fn new(chain_id: u64) -> Self {
        Self {
            paths_cache: Arc::new(RwLock::new(HashMap::new())),
            chain_id,
        }
    }

    pub async fn get_paths_for_pool(
        &self,
        pool: Address,
    ) -> Option<(
        SingleChainPathsWithAnchorToken,
        Vec<SingleChainPathsWithAnchorToken>,
    )> {
        let paths_cache = self.paths_cache.read().await;
        paths_cache.get(&pool).map(|paths| paths.clone())
    }

    pub async fn set_paths(
        &self,
        pool: Address,
        source_path: SingleChainPathsWithAnchorToken,
        target_paths: Vec<SingleChainPathsWithAnchorToken>,
    ) -> Result<()> {
        // Verify Path
        if source_path.paths.is_empty() || target_paths.is_empty() {
            return Err(anyhow::anyhow!("Path is empty"));
        }
        info!(
            "Setting paths for pool: {} on chain {}",
            pool, self.chain_id
        );
        let mut paths_cache = self.paths_cache.write().await;
        // Overwrite existing entry to prevent duplicates - insert() replaces any existing value
        paths_cache.insert(pool, (source_path, target_paths));
        Ok(())
    }
}

/// Helper function to format a path of PoolTokenPair for nice console output
pub fn format_path(path: &[PoolDirection]) -> String {
    if path.is_empty() {
        return "[Empty Path]".to_string();
    }

    let mut result = String::from("");

    // If it's a path, add summary
    if !path.is_empty()
        && path.last().unwrap().token_out == path.first().unwrap().token_in
        && path.len() > 1
    {
        let first_token_str = format!("{:?}", path.first().unwrap().token_in);
        let first_token_short = first_token_str[0..10].to_string();

        let middle_tokens: String = path
            .iter()
            .map(|p| {
                let token_str = format!("{:?}", p.token_out);
                token_str[0..10].to_string()
            })
            .collect::<Vec<String>>()
            .join(" → ");

        result.push_str(&format!(
            "path DETECTED: {} → {}\n",
            first_token_short, middle_tokens
        ));
    }

    result
}

/// Helper function to format a path in a concise way
pub fn format_path_summary(path: &[PoolDirection]) -> String {
    if path.is_empty() {
        return "[Empty path]".to_string();
    }

    // First token info
    let first_token_str = format!("{:?}", path.first().unwrap().token_in);
    let first_token_short = format!(
        "{}..{}",
        &first_token_str[0..6],
        &first_token_str[first_token_str.len() - 4..]
    );

    // Build formatted path showing tokens connected by pools
    let mut formatted_path = vec![first_token_short];

    for pair in path.iter() {
        // Add pool info
        let pool_str = format!("{:?}", pair.pool);
        let pool_short = format!("{}..{}", &pool_str[0..6], &pool_str[pool_str.len() - 4..]);
        formatted_path.push(format!("({})", pool_short));

        // Add token out
        let token_str = format!("{:?}", pair.token_out);
        let token_short = format!(
            "{}..{}",
            &token_str[0..6],
            &token_str[token_str.len() - 4..]
        );
        formatted_path.push(token_short);
    }

    // Join with arrows
    formatted_path.join(" → ")
}
