//! Staging journal and conservative recovery after an interrupted reconciliation.

use super::*;

/// A small, anchored sidecar for private staging directories. It is not a
/// power-loss transaction log; it makes SIGKILL recovery conservative without
/// ever scanning names or writing on unchanged runs.
#[derive(Debug, Default, Deserialize, Serialize)]
pub(super) struct RecoveryJournal {
    #[serde(default)]
    pub(super) version: u32,
    #[serde(
        default,
        serialize_with = "serialize_recovery_artifacts",
        deserialize_with = "deserialize_recovery_artifacts"
    )]
    pub(super) artifacts: BTreeMap<String, RecoveryArtifact>,
    #[serde(
        default,
        serialize_with = "serialize_recovery_fallbacks",
        deserialize_with = "deserialize_recovery_fallbacks"
    )]
    pub(super) fallbacks: BTreeMap<String, FallbackRecovery>,
    #[serde(skip)]
    pub(super) initialized: bool,
    #[serde(skip)]
    pub(super) pending: Vec<RecoveryEvent>,
    #[serde(skip)]
    pub(super) needs_compaction: bool,
    #[serde(skip)]
    pub(super) log_bytes: u64,
    #[serde(skip)]
    pub(super) log_events: usize,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(tag = "operation", rename_all = "snake_case")]
pub(super) enum RecoveryEvent {
    UpsertArtifact { artifact: RecoveryArtifact },
    RemoveArtifact { id: String },
    UpsertFallback { fallback: FallbackRecovery },
    RemoveFallback { stage_id: String },
}

impl RecoveryJournal {
    pub(super) fn push_artifact(&mut self, artifact: RecoveryArtifact) {
        let id = artifact.id.clone();
        assert!(
            !self.artifacts.contains_key(&id),
            "recovery artifact ids are unique"
        );
        self.pending.push(RecoveryEvent::UpsertArtifact {
            artifact: artifact.clone(),
        });
        self.artifacts.insert(id, artifact);
    }

    pub(super) fn mark_artifact(&mut self, id: &str) {
        if let Some(artifact) = self.artifacts.get(id) {
            self.pending.push(RecoveryEvent::UpsertArtifact {
                artifact: artifact.clone(),
            });
        }
    }

    pub(super) fn remove_artifact(&mut self, id: &str) -> bool {
        if self.artifacts.remove(id).is_some() {
            self.pending
                .push(RecoveryEvent::RemoveArtifact { id: id.into() });
            true
        } else {
            false
        }
    }

    pub(super) fn push_fallback(&mut self, fallback: FallbackRecovery) {
        let stage_id = fallback.stage_id.clone();
        assert!(
            !self.fallbacks.contains_key(&stage_id),
            "recovery fallback stage ids are unique"
        );
        self.pending.push(RecoveryEvent::UpsertFallback {
            fallback: fallback.clone(),
        });
        self.fallbacks.insert(stage_id, fallback);
    }

    pub(super) fn mark_fallback(&mut self, stage_id: &str) {
        if let Some(fallback) = self.fallbacks.get(stage_id) {
            self.pending.push(RecoveryEvent::UpsertFallback {
                fallback: fallback.clone(),
            });
        }
    }

    pub(super) fn remove_fallback(&mut self, stage_id: &str) -> bool {
        if self.fallbacks.remove(stage_id).is_some() {
            self.pending.push(RecoveryEvent::RemoveFallback {
                stage_id: stage_id.into(),
            });
            true
        } else {
            false
        }
    }
}

fn serialize_recovery_artifacts<S>(
    artifacts: &BTreeMap<String, RecoveryArtifact>,
    serializer: S,
) -> std::result::Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    serializer.collect_seq(artifacts.values())
}

fn deserialize_recovery_artifacts<'de, D>(
    deserializer: D,
) -> std::result::Result<BTreeMap<String, RecoveryArtifact>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let records = Vec::<RecoveryArtifact>::deserialize(deserializer)?;
    let mut artifacts = BTreeMap::new();
    for artifact in records {
        let id = artifact.id.clone();
        if artifacts.insert(id.clone(), artifact).is_some() {
            return Err(serde::de::Error::custom(format!(
                "duplicate recovery artifact id {id}"
            )));
        }
    }
    Ok(artifacts)
}

fn serialize_recovery_fallbacks<S>(
    fallbacks: &BTreeMap<String, FallbackRecovery>,
    serializer: S,
) -> std::result::Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    serializer.collect_seq(fallbacks.values())
}

fn deserialize_recovery_fallbacks<'de, D>(
    deserializer: D,
) -> std::result::Result<BTreeMap<String, FallbackRecovery>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let records = Vec::<FallbackRecovery>::deserialize(deserializer)?;
    let mut fallbacks = BTreeMap::new();
    for fallback in records {
        let stage_id = fallback.stage_id.clone();
        if fallbacks.insert(stage_id.clone(), fallback).is_some() {
            return Err(serde::de::Error::custom(format!(
                "duplicate recovery fallback stage id {stage_id}"
            )));
        }
    }
    Ok(fallbacks)
}

#[derive(Default)]
pub(super) struct RecoveredArtifacts {
    pub(super) records: Vec<(String, Deferred)>,
    pub(super) live: HashMap<Deferred, Artifact>,
    pub(super) journal_changed: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub(super) struct RecoveryArtifact {
    pub(super) id: String,
    pub(super) intent: ArtifactIntent,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) artifact: Option<Deferred>,
    pub(super) phase: ArtifactPhase,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(super) enum ArtifactPhase {
    Staging,
    Exchanged,
    Backup,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub(super) struct FallbackRecovery {
    pub(super) destination: String,
    pub(super) stage_id: String,
    pub(super) backup_id: String,
    pub(super) original: EntryIdentity,
    pub(super) staged: EntryIdentity,
    pub(super) placeholder: EntryIdentity,
    pub(super) phase: FallbackPhase,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub(super) struct EntryIdentity {
    pub(super) device: u64,
    pub(super) inode: u64,
    pub(super) kind: EntryKind,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(super) enum EntryKind {
    RegularFile,
    Directory,
    Symlink,
    Other,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(super) enum FallbackPhase {
    MayMoveOld,
    MayInstallNew,
}

pub(super) fn read_recovery_anchored(state_directory: &StateDirectory) -> Result<RecoveryJournal> {
    match state_directory.open_recovery() {
        Ok(Some(mut file)) => {
            let encoded_len = file.metadata().into_diagnostic()?.len();
            if encoded_len > MAX_RECOVERY_LOG_READ_BYTES {
                return Err(miette!("files recovery journal exceeds its byte limit"));
            }
            let mut encoded = Vec::with_capacity(encoded_len as usize);
            file.read_to_end(&mut encoded).into_diagnostic()?;
            let mut lines = encoded.split_inclusive(|byte| *byte == b'\n');
            let first = lines
                .next()
                .ok_or_else(|| miette!("files recovery journal is empty"))?;
            let first = first.strip_suffix(b"\n").unwrap_or(first);
            let mut journal: RecoveryJournal = serde_json::from_slice(first)
                .into_diagnostic()
                .wrap_err("refusing to ignore malformed files recovery journal")?;
            let mut event_count = 0;
            let mut needs_compaction = !encoded.ends_with(b"\n");
            for line in lines {
                let terminated = line.ends_with(b"\n");
                let payload = line.strip_suffix(b"\n").unwrap_or(line);
                match serde_json::from_slice::<RecoveryEvent>(payload) {
                    Ok(event) => {
                        apply_recovery_event(&mut journal, event);
                        event_count += 1;
                        if event_count > MAX_RECOVERY_LOG_EVENTS * 4 {
                            return Err(miette!("files recovery journal exceeds its event limit"));
                        }
                        if journal.artifacts.len() > MAX_DEFERRED_CLEANUPS
                            || journal.fallbacks.len() > MAX_DEFERRED_CLEANUPS
                        {
                            return Err(miette!(
                                "files recovery journal exceeds its live-record limit"
                            ));
                        }
                    }
                    Err(error) if !terminated => {
                        // A killed append can leave only its final event torn.
                        // Every mutation is logged before its filesystem step,
                        // so ignoring that incomplete tail is conservative.
                        tracing::warn!(error = %error, "ignoring incomplete files recovery event");
                        needs_compaction = true;
                    }
                    Err(error) => {
                        return Err(error)
                            .into_diagnostic()
                            .wrap_err("refusing to ignore malformed files recovery event");
                    }
                }
            }
            if journal.version > 1 {
                return Err(miette!(
                    "files recovery journal version {} is newer than this runner",
                    journal.version
                ));
            }
            validate_recovery_journal(&journal)?;
            journal.initialized = true;
            journal.pending.clear();
            journal.needs_compaction = needs_compaction
                || encoded_len >= MAX_RECOVERY_LOG_BYTES
                || event_count >= MAX_RECOVERY_LOG_EVENTS;
            journal.log_bytes = encoded_len;
            journal.log_events = event_count;
            Ok(journal)
        }
        Ok(None) => Ok(RecoveryJournal::default()),
        Err(error) => Err(error)
            .into_diagnostic()
            .wrap_err("failed to read files recovery journal"),
    }
}

pub(super) fn apply_recovery_event(journal: &mut RecoveryJournal, event: RecoveryEvent) {
    match event {
        RecoveryEvent::UpsertArtifact { artifact } => {
            journal.artifacts.insert(artifact.id.clone(), artifact);
        }
        RecoveryEvent::RemoveArtifact { id } => {
            journal.artifacts.remove(&id);
        }
        RecoveryEvent::UpsertFallback { fallback } => {
            journal
                .fallbacks
                .insert(fallback.stage_id.clone(), fallback);
        }
        RecoveryEvent::RemoveFallback { stage_id } => {
            journal.fallbacks.remove(&stage_id);
        }
    }
}

pub(super) fn validate_recovery_journal(journal: &RecoveryJournal) -> Result<()> {
    if journal.artifacts.is_empty() && journal.fallbacks.is_empty() {
        return Ok(());
    }
    if journal.version != 1 {
        return Err(miette!(
            "non-empty files recovery journal must use version 1"
        ));
    }
    if journal.artifacts.len() > MAX_DEFERRED_CLEANUPS
        || journal.fallbacks.len() > MAX_DEFERRED_CLEANUPS
    {
        return Err(miette!("files recovery journal exceeds its safety limit"));
    }

    for (artifact_id, artifact) in &journal.artifacts {
        if artifact_id != &artifact.id {
            return Err(miette!("files recovery artifact key does not match its id"));
        }
        let id = Uuid::parse_str(&artifact.id)
            .into_diagnostic()
            .wrap_err("files recovery artifact has an invalid id")?;
        let expected_name = format!(".devenv-files-artifact-{id}");
        if artifact.intent.name != expected_name {
            return Err(miette!(
                "files recovery artifact name does not match its id"
            ));
        }
        if artifact.intent.relative_parent.is_absolute()
            || artifact
                .intent
                .relative_parent
                .components()
                .any(|component| !matches!(component, Component::Normal(_)))
        {
            return Err(miette!(
                "files recovery artifact parent is not a clean relative path"
            ));
        }
        if let Some(deferred) = &artifact.artifact
            && (deferred.relative_parent != artifact.intent.relative_parent
                || deferred.parent_device != artifact.intent.parent_device
                || deferred.parent_inode != artifact.intent.parent_inode
                || deferred.name != artifact.intent.name)
        {
            return Err(miette!(
                "files recovery artifact identity does not match its intent"
            ));
        }
    }

    let mut referenced = HashSet::with_capacity(journal.fallbacks.len() * 2);
    for (fallback_id, fallback) in &journal.fallbacks {
        if fallback_id != &fallback.stage_id {
            return Err(miette!(
                "files recovery fallback key does not match its stage id"
            ));
        }
        validate_relative(&fallback.destination)?;
        if fallback.stage_id == fallback.backup_id
            || !referenced.insert(fallback.stage_id.as_str())
            || !referenced.insert(fallback.backup_id.as_str())
        {
            return Err(miette!(
                "files recovery fallback contains duplicate artifact references"
            ));
        }
        let Some(stage) = journal.artifacts.get(fallback.stage_id.as_str()) else {
            return Err(miette!("files recovery fallback stage is missing"));
        };
        let Some(backup) = journal.artifacts.get(fallback.backup_id.as_str()) else {
            return Err(miette!("files recovery fallback backup is missing"));
        };
        if stage.phase != ArtifactPhase::Staging || backup.phase != ArtifactPhase::Backup {
            return Err(miette!("files recovery fallback artifact phase is invalid"));
        }
        if stage.artifact.is_none() || backup.artifact.is_none() {
            return Err(miette!(
                "files recovery fallback is missing an artifact identity"
            ));
        }
        let destination_parent = Path::new(&fallback.destination)
            .parent()
            .unwrap_or_else(|| Path::new(""));
        if stage.intent.relative_parent != destination_parent
            || backup.intent.relative_parent != destination_parent
            || stage.intent.parent_device != backup.intent.parent_device
            || stage.intent.parent_inode != backup.intent.parent_inode
        {
            return Err(miette!(
                "files recovery fallback artifacts do not share the destination parent"
            ));
        }
    }
    Ok(())
}

pub(super) fn write_recovery(
    state_directory: &StateDirectory,
    journal: &mut RecoveryJournal,
) -> Result<()> {
    if journal.artifacts.is_empty() && journal.fallbacks.is_empty() {
        state_directory.remove_recovery().into_diagnostic()?;
        journal.initialized = false;
        journal.pending.clear();
        journal.needs_compaction = false;
        journal.log_bytes = 0;
        journal.log_events = 0;
        return Ok(());
    }
    if journal.initialized
        && !journal.needs_compaction
        && journal.log_bytes < MAX_RECOVERY_LOG_BYTES
        && journal.log_events < MAX_RECOVERY_LOG_EVENTS
    {
        if journal.pending.is_empty() {
            return Ok(());
        }
        // Every written prefix stays structurally valid: referenced
        // artifacts appear before fallbacks, and fallbacks disappear before
        // their artifacts.
        journal.pending.sort_by_key(|event| match event {
            RecoveryEvent::UpsertArtifact { .. } => 0,
            RecoveryEvent::UpsertFallback { .. } => 1,
            RecoveryEvent::RemoveFallback { .. } => 2,
            RecoveryEvent::RemoveArtifact { .. } => 3,
        });
        let mut file = match state_directory.append_recovery() {
            Ok(file) => file,
            Err(error) => {
                journal.needs_compaction = true;
                return Err(error)
                    .into_diagnostic()
                    .wrap_err("failed to append files recovery journal");
            }
        };
        let mut appended_bytes = 0_u64;
        for event in &journal.pending {
            let encoded = serde_json::to_vec(event).into_diagnostic()?;
            appended_bytes += encoded.len() as u64 + 1;
            if let Err(error) = file
                .write_all(&encoded)
                .and_then(|()| file.write_all(b"\n"))
            {
                journal.needs_compaction = true;
                return Err(error)
                    .into_diagnostic()
                    .wrap_err("failed to append files recovery event");
            }
        }
        journal.log_bytes += appended_bytes;
        journal.log_events += journal.pending.len();
        journal.pending.clear();
        return Ok(());
    }
    let (mut temporary, name) = state_directory.recovery_temporary().into_diagnostic()?;
    let write_result = serde_json::to_writer(&mut temporary, journal)
        .into_diagnostic()
        .and_then(|()| temporary.write_all(b"\n").into_diagnostic());
    if let Err(error) = write_result {
        state_directory.remove_temporary(&name);
        return Err(error);
    }
    let compacted_bytes = temporary.metadata().into_diagnostic()?.len();
    if let Err(error) = state_directory.replace_recovery(&name) {
        state_directory.remove_temporary(&name);
        return Err(error)
            .into_diagnostic()
            .wrap_err("failed to replace files recovery journal");
    }
    journal.initialized = true;
    journal.pending.clear();
    journal.needs_compaction = false;
    journal.log_bytes = compacted_bytes;
    journal.log_events = 0;
    Ok(())
}

pub(super) fn recover_artifacts(
    state_directory: &StateDirectory,
    resolver: &mut Resolver,
    journal: &mut RecoveryJournal,
    cancellation: &CancellationToken,
) -> Result<RecoveredArtifacts> {
    if journal.artifacts.is_empty() && journal.fallbacks.is_empty() {
        return Ok(RecoveredArtifacts {
            journal_changed: journal.initialized,
            ..RecoveredArtifacts::default()
        });
    }
    let mut recovered_fallbacks = HashSet::new();
    for fallback in journal.fallbacks.values() {
        match recover_fallback(resolver, journal, fallback, cancellation) {
            Ok(true) => {
                recovered_fallbacks.insert(fallback.stage_id.clone());
            }
            Ok(false) => {
                return Err(miette!(
                    "cannot safely recover interrupted replacement for {}; recovery journal: {}. Inspect the destination and its .devenv-files-artifact-* directories first. Only after restoring them or accepting their loss, remove the recovery journal to resume reconciliation.",
                    fallback.destination,
                    state_directory.recovery_path().display(),
                ));
            }
            Err(error) => {
                return Err(error).into_diagnostic().wrap_err_with(|| {
                    format!(
                        "could not recover interrupted replacement for {}",
                        fallback.destination
                    )
                });
            }
        }
    }
    if !recovered_fallbacks.is_empty() {
        for id in &recovered_fallbacks {
            journal.remove_fallback(id);
        }
    }
    // Fallback records are handled as a pair. Do not sweep either
    // independently until their phase/identities made a conclusion safe.
    let protected: HashSet<_> = journal
        .fallbacks
        .values()
        .flat_map(|fallback| [&fallback.stage_id, &fallback.backup_id])
        .collect();
    let mut removed = HashSet::new();
    let mut recovered = RecoveredArtifacts {
        journal_changed: journal.needs_compaction || !recovered_fallbacks.is_empty(),
        ..RecoveredArtifacts::default()
    };
    for artifact in journal.artifacts.values_mut() {
        if protected.contains(&artifact.id) {
            continue;
        }
        let deferred = match &artifact.artifact {
            Some(deferred) => deferred.clone(),
            None => match resolver
                .open_artifact_intent(&artifact.intent)
                .into_diagnostic()?
            {
                ArtifactIntentResolution::Absent => {
                    removed.insert(artifact.id.clone());
                    recovered.journal_changed = true;
                    continue;
                }
                ArtifactIntentResolution::Open(open) => {
                    // The journaled UUID name and anchored parent identify an
                    // artifact created just before its inode could be saved.
                    let deferred = open.deferred();
                    artifact.artifact = Some(deferred.clone());
                    recovered
                        .records
                        .push((artifact.id.clone(), deferred.clone()));
                    if recovered.live.len() < MAX_DEFERRED_CLEANUP_RECORDS_PER_RUN {
                        recovered.live.insert(deferred.clone(), open);
                    }
                    recovered.journal_changed = true;
                    continue;
                }
                ArtifactIntentResolution::IdentityMismatch => {
                    // No fallback can reference an intent-only artifact. If
                    // its anchored name changed, abandoning it is safer than
                    // blocking reconciliation or guessing what now owns it.
                    tracing::warn!(artifact = %artifact.id, "abandoning changed intent-only files artifact");
                    removed.insert(artifact.id.clone());
                    recovered.journal_changed = true;
                    continue;
                }
            },
        };
        recovered.records.push((artifact.id.clone(), deferred));
    }
    if !removed.is_empty() {
        for id in &removed {
            journal.remove_artifact(id);
        }
    }
    for (id, _) in &recovered.records {
        if journal
            .artifacts
            .get(id)
            .is_some_and(|artifact| artifact.artifact.is_some())
        {
            journal.mark_artifact(id);
        }
    }
    Ok(recovered)
}

pub(super) fn publish_recovered_artifacts(
    state_directory: &StateDirectory,
    state: &mut FilesState,
    journal: &mut RecoveryJournal,
    recovered: RecoveredArtifacts,
) -> Result<HashMap<Deferred, Artifact>> {
    let mut known: HashSet<_> = state.garbage.iter().cloned().collect();
    let mut state_changed = false;
    for (_, deferred) in &recovered.records {
        if known.insert(deferred.clone()) {
            state.garbage.push(deferred.clone());
            state_changed = true;
        }
    }
    // State first: once this succeeds, bounded cleanup owns every exact
    // identity even if pruning the recovery sidecar is interrupted.
    if state_changed {
        write_state(state_directory, state)
            .wrap_err("failed to publish recovered files cleanup work")?;
    }

    let recovered_ids: HashSet<_> = recovered
        .records
        .iter()
        .map(|(id, _)| id.as_str())
        .collect();
    if !recovered_ids.is_empty() {
        for id in &recovered_ids {
            journal.remove_artifact(id);
        }
    }
    if (recovered.journal_changed || !recovered_ids.is_empty())
        && let Err(error) = write_recovery(state_directory, journal)
    {
        // The state already owns all exact cleanup identities. Retrying a
        // stale sidecar next run is safe and must not block reconciliation.
        tracing::warn!(error = %error, "could not prune recovered files artifacts");
    }
    Ok(recovered.live)
}

pub(super) fn entry_kind(kind: rustix::fs::FileType) -> EntryKind {
    if kind == rustix::fs::FileType::RegularFile {
        EntryKind::RegularFile
    } else if kind == rustix::fs::FileType::Directory {
        EntryKind::Directory
    } else if kind == rustix::fs::FileType::Symlink {
        EntryKind::Symlink
    } else {
        EntryKind::Other
    }
}

pub(super) fn entry_identity(metadata: destination::EntryMetadata) -> EntryIdentity {
    EntryIdentity {
        device: metadata.device,
        inode: metadata.inode,
        kind: entry_kind(metadata.kind),
    }
}

pub(super) fn matches_identity(
    metadata: Option<destination::EntryMetadata>,
    expected: EntryIdentity,
) -> bool {
    metadata.is_some_and(|metadata| {
        metadata.device == expected.device
            && metadata.inode == expected.inode
            && entry_kind(metadata.kind) == expected.kind
    })
}

pub(super) fn recover_fallback(
    resolver: &mut Resolver,
    journal: &RecoveryJournal,
    fallback: &FallbackRecovery,
    cancellation: &CancellationToken,
) -> std::io::Result<bool> {
    if cancellation.is_cancelled() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::Interrupted,
            "file recovery cancelled",
        ));
    }
    let destination = resolver.destination(&fallback.destination, false)?;
    let destination_state = destination.metadata()?;
    if matches_identity(destination_state, fallback.original)
        || matches_identity(destination_state, fallback.staged)
    {
        // Either the last published destination or the fully installed new
        // destination is present. Both are coherent starting points for the
        // ordinary reconciliation that follows.
        return Ok(true);
    }
    if destination_state.is_some() {
        return Ok(false);
    }

    let Some(stage_record) = journal.artifacts.get(&fallback.stage_id) else {
        return Ok(false);
    };
    let Some(backup_record) = journal.artifacts.get(&fallback.backup_id) else {
        return Ok(false);
    };
    let (Some(stage_deferred), Some(backup_deferred)) =
        (&stage_record.artifact, &backup_record.artifact)
    else {
        return Ok(false);
    };
    let Some(stage) = resolver.open_artifact(stage_deferred)? else {
        return Ok(false);
    };
    let Some(backup) = resolver.open_artifact(backup_deferred)? else {
        return Ok(false);
    };
    let stage_state = stage.payload_identity()?;
    let backup_state = backup.payload_identity()?;
    let backup_is_old = matches_identity(backup_state, fallback.original);
    let stage_is_new = matches_identity(stage_state, fallback.staged);
    let expected_old = destination::EntryMetadata {
        kind: match fallback.original.kind {
            EntryKind::RegularFile => rustix::fs::FileType::RegularFile,
            EntryKind::Directory => rustix::fs::FileType::Directory,
            EntryKind::Symlink => rustix::fs::FileType::Symlink,
            EntryKind::Other => return Ok(false),
        },
        device: fallback.original.device,
        inode: fallback.original.inode,
    };

    match fallback.phase {
        FallbackPhase::MayMoveOld | FallbackPhase::MayInstallNew
            if backup_is_old && stage_is_new =>
        {
            backup.restore_payload_to(&destination, expected_old)
        }
        FallbackPhase::MayMoveOld | FallbackPhase::MayInstallNew => Ok(false),
    }
}

pub(super) fn forget_persisted_recovery(
    state_directory: &StateDirectory,
    recovery: &mut RecoveryJournal,
    garbage: &[Deferred],
) -> Result<()> {
    if recovery.artifacts.is_empty() || garbage.is_empty() {
        return Ok(());
    }
    let persisted: HashSet<_> = garbage.iter().collect();
    let removed_artifacts = recovery
        .artifacts
        .values()
        .filter(|artifact| {
            artifact
                .artifact
                .as_ref()
                .is_some_and(|deferred| persisted.contains(deferred))
        })
        .map(|artifact| artifact.id.clone())
        .collect::<HashSet<_>>();
    if !removed_artifacts.is_empty() {
        let retained_ids: HashSet<_> = recovery
            .artifacts
            .values()
            .filter(|artifact| !removed_artifacts.contains(&artifact.id))
            .map(|artifact| artifact.id.as_str())
            .collect();
        let removed_fallbacks = recovery
            .fallbacks
            .values()
            .filter(|fallback| {
                !retained_ids.contains(fallback.stage_id.as_str())
                    || !retained_ids.contains(fallback.backup_id.as_str())
            })
            .map(|fallback| fallback.stage_id.clone())
            .collect::<Vec<_>>();
        for id in removed_fallbacks {
            recovery.remove_fallback(&id);
        }
        for id in removed_artifacts {
            recovery.remove_artifact(&id);
        }
        write_recovery(state_directory, recovery)?;
    }
    Ok(())
}
