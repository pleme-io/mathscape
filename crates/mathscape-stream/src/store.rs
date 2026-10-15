use crate::error::StreamError;
use mathscape_core::hash::TermRef;
use mathscape_core::snapshot::ModelSnapshot;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

const TMP_PREFIX: &str = ".tmp-";
const HEAD: &str = "HEAD";
const HEAD_TMP: &str = "HEAD.tmp";

#[derive(Debug, Clone)]
pub struct ObjectStore {
    root: PathBuf,
}

#[derive(Debug)]
pub struct Staged {
    tmp: PathBuf,
    hash: TermRef,
}

impl Staged {
    #[must_use]
    pub fn hash(&self) -> TermRef {
        self.hash
    }
}

#[derive(Debug)]
pub struct StagedHead {
    tmp: PathBuf,
    hash: TermRef,
}

fn sync_dir(dir: &Path) -> Result<(), StreamError> {
    File::open(dir)?.sync_all()?;
    Ok(())
}

fn write_synced(path: &Path, bytes: &[u8]) -> Result<(), StreamError> {
    let mut f = File::create(path)?;
    f.write_all(bytes)?;
    f.sync_all()?;
    Ok(())
}

impl ObjectStore {
    pub fn open(root: impl Into<PathBuf>) -> Result<Self, StreamError> {
        let root = root.into();
        fs::create_dir_all(root.join("objects"))?;
        let store = Self { root };
        store.sweep()?;
        Ok(store)
    }

    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    fn sweep(&self) -> Result<(), StreamError> {
        let head_tmp = self.root.join(HEAD_TMP);
        if head_tmp.exists() {
            fs::remove_file(head_tmp)?;
        }
        for shard in fs::read_dir(self.root.join("objects"))? {
            let shard = shard?.path();
            if !shard.is_dir() {
                continue;
            }
            for entry in fs::read_dir(&shard)? {
                let p = entry?.path();
                let stale = p
                    .file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| n.starts_with(TMP_PREFIX));
                if stale {
                    fs::remove_file(p)?;
                }
            }
        }
        Ok(())
    }

    #[must_use]
    pub fn object_path(&self, hash: &TermRef) -> PathBuf {
        let hex = hash.to_hex();
        self.root.join("objects").join(&hex[..2]).join(&hex[2..])
    }

    fn tmp_path(&self, hash: &TermRef) -> Result<PathBuf, StreamError> {
        let hex = hash.to_hex();
        let shard = self.root.join("objects").join(&hex[..2]);
        fs::create_dir_all(&shard)?;
        Ok(shard.join(format!("{TMP_PREFIX}{}", &hex[2..])))
    }

    #[must_use]
    pub fn contains(&self, hash: &TermRef) -> bool {
        self.object_path(hash).exists()
    }

    pub fn put_bytes(&self, bytes: &[u8]) -> Result<TermRef, StreamError> {
        let hash = TermRef::from_bytes(bytes);
        if self.contains(&hash) {
            return Ok(hash);
        }
        let tmp = self.tmp_path(&hash)?;
        write_synced(&tmp, bytes)?;
        self.commit_object(Staged { tmp, hash })
    }

    pub fn get_bytes(&self, hash: &TermRef) -> Result<Vec<u8>, StreamError> {
        let bytes = fs::read(self.object_path(hash))?;
        let actual = TermRef::from_bytes(&bytes);
        if actual != *hash {
            return Err(StreamError::Corrupt {
                object: hash.to_hex(),
                reason: format!("bytes hash to {}", actual.to_hex()),
            });
        }
        Ok(bytes)
    }

    pub fn stage_snapshot(&self, snap: &mut ModelSnapshot) -> Result<Staged, StreamError> {
        snap.content_hash = snap.compute_content_hash();
        let hash = TermRef(snap.content_hash);
        let tmp = self.tmp_path(&hash)?;
        snap.save_to_path(&tmp)?;
        File::open(&tmp)?.sync_all()?;
        Ok(Staged { tmp, hash })
    }

    pub fn commit_object(&self, staged: Staged) -> Result<TermRef, StreamError> {
        let dest = self.object_path(&staged.hash);
        fs::rename(&staged.tmp, &dest)?;
        if let Some(parent) = dest.parent() {
            sync_dir(parent)?;
        }
        Ok(staged.hash)
    }

    pub fn put_snapshot(&self, snap: &mut ModelSnapshot) -> Result<TermRef, StreamError> {
        let staged = self.stage_snapshot(snap)?;
        self.commit_object(staged)
    }

    pub fn get_snapshot(&self, hash: &TermRef) -> Result<ModelSnapshot, StreamError> {
        let path = self.object_path(hash);
        if !path.exists() {
            return Err(StreamError::Missing { object: hash.to_hex() });
        }
        let snap = ModelSnapshot::load_from_path(&path).map_err(|e| StreamError::Corrupt {
            object: hash.to_hex(),
            reason: e.to_string(),
        })?;
        if snap.content_hash != hash.0 {
            return Err(StreamError::Corrupt {
                object: hash.to_hex(),
                reason: format!("stored under {} but its content hash is {}", hash.to_hex(), TermRef(snap.content_hash).to_hex()),
            });
        }
        Ok(snap)
    }

    pub fn head(&self) -> Result<Option<TermRef>, StreamError> {
        let path = self.root.join(HEAD);
        if !path.exists() {
            return Ok(None);
        }
        let text = fs::read_to_string(&path)?;
        TermRef::from_hex(text.trim())
            .map(Some)
            .ok_or_else(|| StreamError::BadHead { content: text.trim().to_owned() })
    }

    pub fn stage_head(&self, hash: TermRef) -> Result<StagedHead, StreamError> {
        let tmp = self.root.join(HEAD_TMP);
        write_synced(&tmp, format!("{}\n", hash.to_hex()).as_bytes())?;
        Ok(StagedHead { tmp, hash })
    }

    pub fn commit_head(&self, staged: StagedHead) -> Result<TermRef, StreamError> {
        fs::rename(&staged.tmp, self.root.join(HEAD))?;
        sync_dir(&self.root)?;
        Ok(staged.hash)
    }

    pub fn set_head(&self, hash: TermRef) -> Result<TermRef, StreamError> {
        let staged = self.stage_head(hash)?;
        self.commit_head(staged)
    }

    pub fn append_line(&self, file: &str, line: &str) -> Result<(), StreamError> {
        let mut f = OpenOptions::new().create(true).append(true).open(self.root.join(file))?;
        f.write_all(line.as_bytes())?;
        f.write_all(b"\n")?;
        f.sync_all()?;
        Ok(())
    }

    pub fn read_lines(&self, file: &str) -> Result<Vec<String>, StreamError> {
        let path = self.root.join(file);
        if !path.exists() {
            return Ok(Vec::new());
        }
        let text = fs::read_to_string(&path)?;
        let complete = text.rfind('\n').map_or(0, |i| i + 1);
        if complete < text.len() {
            let f = OpenOptions::new().write(true).open(&path)?;
            f.set_len(complete as u64)?;
            f.sync_all()?;
        }
        Ok(text[..complete].lines().map(str::to_owned).collect())
    }
}
