/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

//! # commits-trait
//!
//! Abstractions of the commit (graph + text) operations.
//!
//! The `dag` crate only handles the commit graph without storing the commit
//! text. This crate builds on top of `dag` to handle the commit text. If you
//! are looking for commit author/date/message/root trees. This create provides
//! them. For the graph related logic, like finding ancestors, this crate simply
//! delegates to `dag`.

use std::io;
use std::sync::Arc;

use anyhow::bail;
use dag::CloneData;
use dag::DagAlgorithm;
use dag::Set;
use dag::Vertex;
use dag::VertexListWithOptions;
use dag::errors::NotFoundError;
use dag::ops::CheckIntegrity;
use dag::ops::IdConvert;
use dag::ops::IdMapSnapshot;
use dag::ops::PrefixLookup;
use dag::ops::ToIdSet;
use dag::ops::ToSet;
use format_util::CommitFields;
use format_util::commit_text_to_fields;
use futures::future::try_join_all;
use futures::stream::BoxStream;
use metalog::MetaLog;
use minibytes::Bytes;
use serde::Deserialize;
use serde::Serialize;
use storemodel::ReadRootTreeIds;
use storemodel::SerializationFormat;

#[async_trait::async_trait]
pub trait ReadCommitText: Sync {
    /// Read raw text for a commit, in hg commit format.
    async fn get_commit_raw_text(&self, vertex: &Vertex) -> Result<Option<Bytes>> {
        let list = self.get_commit_raw_text_list(&[vertex.clone()]).await?;
        Ok(Some(list.into_iter().next().unwrap()))
    }

    /// Read commit text in batch. Any of the missing commits would cause an error.
    async fn get_commit_raw_text_list(&self, vertexes: &[Vertex]) -> Result<Vec<Bytes>> {
        try_join_all(vertexes.iter().map(|v| async move {
            match self.get_commit_raw_text(v).await {
                Err(e) => Err(e),
                Ok(None) => v.not_found().map_err(|e| e.into()),
                Ok(Some(b)) => Ok(b),
            }
        }))
        .await
    }

    /// Returns parsed fields of a commit.
    async fn get_commit_fields(&self, vertex: &Vertex) -> Result<Option<Box<dyn CommitFields>>> {
        let text = match self.get_commit_raw_text(vertex).await? {
            None => return Ok(None),
            Some(text) => text,
        };
        // Matches the Python-side behavior: silently accept non-utf8 text.
        let text = text.into_text_lossy();
        let format = self.format();
        let fields = commit_text_to_fields(text, format);
        Ok(Some(fields))
    }

    /// Read commits in batch and parse them into `CommitFields`.
    async fn get_commit_fields_list(
        &self,
        vertexes: &[Vertex],
    ) -> Result<Vec<Box<dyn CommitFields>>> {
        let texts = self.get_commit_raw_text_list(vertexes).await?;
        let format = self.format();
        let fields: Vec<_> = texts
            .into_iter()
            .map(|bytes| {
                // Matches the Python-side behavior: silently accept non-utf8 text.
                let text = bytes.into_text_lossy();
                commit_text_to_fields(text, format)
            })
            .collect();
        Ok(fields)
    }

    /// Return a trait object that is easier to be passed around.
    fn to_dyn_read_commit_text(&self) -> Arc<dyn ReadCommitText + Send + Sync>;

    /// Return a trait object to resolve root tree ids from commit ids.
    fn to_dyn_read_root_tree_ids(&self) -> Arc<dyn ReadRootTreeIds + Send + Sync> {
        let reader = self.to_dyn_read_commit_text();
        let reader = trait_impls::ArcReadCommitText(reader);
        Arc::new(reader)
    }

    fn format(&self) -> SerializationFormat;
}

pub trait StreamCommitText {
    /// Get commit raw text in a stream fashion.
    fn stream_commit_raw_text(
        &self,
        stream: BoxStream<'static, anyhow::Result<Vertex>>,
    ) -> Result<BoxStream<'static, anyhow::Result<ParentlessHgCommit>>>;
}

#[async_trait::async_trait]
pub trait AppendCommits: Send + Sync {
    /// Add commits. They stay in-memory until `flush`.
    async fn add_commits(&mut self, commits: &[HgCommit]) -> Result<()>;

    /// Write in-memory changes to disk.
    ///
    /// This function does more things than `flush_commit_data`.
    async fn flush(&mut self, master_heads: &[Vertex]) -> Result<()>;

    /// Write buffered commit data to disk.
    ///
    /// For the revlog backend, this also write the commit graph to disk.
    async fn flush_commit_data(&mut self) -> Result<()>;

    /// Add nodes to the graph without data (commit message).
    /// This is only supported by lazy backends.
    /// Use `flush` to write changes to disk.
    async fn add_graph_nodes(&mut self, graph_nodes: &[GraphNode]) -> Result<()> {
        let _ = graph_nodes;
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "add_graph_nodes is not supported by this backend",
        )
        .into())
    }

    /// Import clone data and flush.
    /// This is only supported by lazy backends and can only be used in an empty repo.
    async fn import_clone_data(&mut self, clone_data: CloneData<Vertex>) -> Result<()> {
        let _ = clone_data;
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "import_clone_data is not supported by this backend",
        )
        .into())
    }

    /// Import data from master fast forward pull.
    /// This is only supported by lazy backends. Can be used on non-empty repo.
    async fn import_pull_data(
        &mut self,
        clone_data: CloneData<Vertex>,
        heads: &VertexListWithOptions,
    ) -> Result<()> {
        let _ = (clone_data, heads);
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "import_pull_data is not supported by this backend",
        )
        .into())
    }

    /// Update references to match metalog.
    ///
    /// This is not needed if metalog is the source of truth.
    /// However, if metalog is synced from git references, then this
    /// method is needed to sync metalog back to git references.
    fn update_references_to_match_metalog(&mut self, metalog: &MetaLog) -> Result<()> {
        let _ = metalog;
        Ok(())
    }

    /// Import external (e.g. git) references to metalog and import related
    /// commits to the dag index.
    ///
    /// This can be useful to pick up changes made by external program
    /// (e.g. `git fetch`) explicitly.
    fn import_external_references(
        &mut self,
        _metalog: &mut MetaLog,
        _names: &[String],
    ) -> Result<()> {
        bail!("This commit backend does not support 'import_external_reference'");
    }

    /// Update virtual nodes like `wdir()` and `null()`.
    /// `null()` and `wdir()` will have Id mapped to the first 2 ids in the virtual group.
    async fn update_virtual_nodes(&mut self, wdir_parents: Vec<Vertex>) -> Result<()>;
}

pub trait DescribeBackend {
    /// Name of the DagAlgorithm backend.
    fn algorithm_backend(&self) -> &'static str;

    /// Describe what storage backend is being used.
    fn describe_backend(&self) -> String;

    /// Write human-readable internal data to `w`.
    /// For segments backend, this writes segments data.
    fn explain_internals(&self, w: &mut dyn io::Write) -> io::Result<()>;
}

#[async_trait::async_trait]
pub trait StripCommits {
    /// Strip commits. This is for legacy tests only that wouldn't be used
    /// much in production. The callsite should take care of locking or
    /// otherwise risk data race and loss.
    async fn strip_commits(&mut self, set: Set) -> Result<()>;
}

/// A combination of other traits: commit read/write + DAG algorithms.
pub trait DagCommits:
    ReadCommitText
    + StripCommits
    + AppendCommits
    + CheckIntegrity
    + DescribeBackend
    + DagAlgorithm
    + IdConvert
    + IdMapSnapshot
    + PrefixLookup
    + ToIdSet
    + ToSet
{
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GraphNode {
    pub vertex: Vertex,
    pub parents: Vec<Vertex>,
}

/// Parameter used by `add_commits`.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct HgCommit {
    pub vertex: Vertex,
    pub parents: Vec<Vertex>,
    pub raw_text: Bytes,
}

/// Return type used by `stream_commit_raw_text`.
#[derive(Serialize, Deserialize, Debug)]
pub struct ParentlessHgCommit {
    pub vertex: Vertex,
    pub raw_text: Bytes,
}

pub mod trait_impls;

pub use anyhow::Result;
