/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::Arc;

use bookmarks::BookmarkKey;
use context::CoreContext;
use fbinit::FacebookInit;
use fixtures::TestRepoFixture;
use hook_manager::HookRepo;
use hook_manager_testlib::HookTestRepo;
use maplit::hashset;
use metaconfig_types::BookmarkParams;
use metaconfig_types::HookManagerParams;
use metaconfig_types::HookParams;
use metaconfig_types::RepoConfig;
use mononoke_macros::mononoke;
use permission_checker::InternalAclProvider;
use repo_permission_checker::NeverAllowRepoPermissionChecker;
use scuba_ext::MononokeScubaSampleBuilder;

use crate::HookManager;
use crate::errors::ErrorKind;
use crate::hook_loader::load_hooks;

async fn hook_manager_repo(fb: FacebookInit, repo: &HookTestRepo) -> HookManager {
    let ctx = CoreContext::test_mock(fb);

    let hook_repo = HookRepo::build_from(&repo);
    HookManager::new(
        ctx.fb,
        &InternalAclProvider::default(),
        hook_repo,
        HookManagerParams {
            disable_acl_checker: true,
            ..Default::default()
        },
        Arc::new(NeverAllowRepoPermissionChecker {}),
        MononokeScubaSampleBuilder::with_discard(),
        "zoo".to_string(),
    )
    .await
    .expect("Failed to construct HookManager")
}

async fn hook_manager_many_files_dirs_repo(fb: FacebookInit) -> HookManager {
    hook_manager_repo(fb, &fixtures::ManyFilesDirs::get_repo(fb).await).await
}

#[mononoke::fbinit_test]
async fn test_load_hooks_bad_rust_hook(fb: FacebookInit) {
    let mut config = RepoConfig::default();
    config.bookmarks = vec![BookmarkParams {
        bookmark: BookmarkKey::new("bm1").unwrap().into(),
        hooks: vec!["hook1".into()],
        only_fast_forward: false,
        allowed_users: None,
        allowed_hipster_group: None,
        rewrite_dates: None,
        hooks_skip_ancestors_of: vec![],
        ensure_ancestor_of: None,
        allow_move_to_public_commits_without_hooks: false,
    }];

    config.hooks = vec![HookParams {
        name: "hook1".into(),
        implementation: "hook1".into(),
        config: Default::default(),
    }];

    let mut hm = hook_manager_many_files_dirs_repo(fb).await;

    match load_hooks(
        fb,
        &InternalAclProvider::default(),
        &mut hm,
        &config,
        &hashset![],
    )
    .await
    .unwrap_err()
    .downcast::<ErrorKind>()
    {
        Ok(ErrorKind::InvalidRustHook(hook_name)) => {
            assert_eq!(hook_name, "hook1".to_string());
        }
        _ => panic!("Unexpected err type"),
    };
}

#[mononoke::fbinit_test]
async fn test_load_disabled_hooks(fb: FacebookInit) {
    let mut config = RepoConfig::default();

    config.hooks = vec![HookParams {
        name: "hook1".into(),
        implementation: "hook1".into(),
        config: Default::default(),
    }];

    let mut hm = hook_manager_many_files_dirs_repo(fb).await;

    load_hooks(
        fb,
        &InternalAclProvider::default(),
        &mut hm,
        &config,
        &hashset!["hook1".to_string()],
    )
    .await
    .expect("disabling a broken hook should allow loading to succeed");
}

#[mononoke::fbinit_test]
async fn test_load_disabled_hooks_referenced_by_bookmark(fb: FacebookInit) {
    let mut config = RepoConfig::default();

    config.bookmarks = vec![BookmarkParams {
        bookmark: BookmarkKey::new("bm1").unwrap().into(),
        hooks: vec!["hook1".into()],
        only_fast_forward: false,
        allowed_users: None,
        allowed_hipster_group: None,
        rewrite_dates: None,
        hooks_skip_ancestors_of: vec![],
        ensure_ancestor_of: None,
        allow_move_to_public_commits_without_hooks: false,
    }];

    config.hooks = vec![HookParams {
        name: "hook1".into(),
        implementation: "hook1".into(),
        config: Default::default(),
    }];

    let mut hm = hook_manager_many_files_dirs_repo(fb).await;

    load_hooks(
        fb,
        &InternalAclProvider::default(),
        &mut hm,
        &config,
        &hashset!["hook1".to_string()],
    )
    .await
    .expect("disabling a broken hook should allow loading to succeed");
}

#[mononoke::fbinit_test]
async fn test_load_disabled_hooks_hook_does_not_exist(fb: FacebookInit) {
    let config = RepoConfig::default();
    let mut hm = hook_manager_many_files_dirs_repo(fb).await;

    match load_hooks(
        fb,
        &InternalAclProvider::default(),
        &mut hm,
        &config,
        &hashset!["hook1".to_string()],
    )
    .await
    .unwrap_err()
    .downcast::<ErrorKind>()
    {
        Ok(ErrorKind::NoSuchHookToDisable(hooks)) => {
            assert_eq!(hashset!["hook1".to_string()], hooks);
        }
        _ => panic!("Unexpected err type"),
    };
}

#[mononoke::fbinit_test]
async fn test_load_hook_with_different_name_and_implementation(fb: FacebookInit) {
    let mut config = RepoConfig::default();

    config.hooks = vec![HookParams {
        name: "hook1".into(),
        implementation: "always_fail_changeset".into(),
        config: Default::default(),
    }];

    let mut hm = hook_manager_many_files_dirs_repo(fb).await;

    load_hooks(
        fb,
        &InternalAclProvider::default(),
        &mut hm,
        &config,
        &hashset![],
    )
    .await
    .expect("loading hooks should succeed");
}
