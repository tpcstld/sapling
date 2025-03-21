# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

# This test represents the scenario in https://internalfb.com/excalidraw/EX180556
  $ . "${TEST_FIXTURES}/library.sh"
  $ REPOTYPE="blob_files"
  $ setup_common_config $REPOTYPE
  $ GIT_REPO_ORIGIN="${TESTTMP}/origin/repo-git"
  $ GIT_REPO="${TESTTMP}/repo-git"

# Setup git repository
  $ mkdir -p "$GIT_REPO_ORIGIN"
  $ cd "$GIT_REPO_ORIGIN"
  $ git init -q
  $ echo "this is fileA" > fileA
  $ git add fileA
  $ git commit -qam "Add fileA"
  $ git tag -a -m"new tag" first_tag
  $ echo "this is fileA.1" > fileA
  $ echo "this is fileB.this is fileB.this is fileB.this is fileB.this is fileB.this is fileB.this is fileB.this is fileB" > fileB
  $ git add .
  $ git commit -qam "Modified fileA -> fileA.1, Add fileB"
  $ git tag -a empty_tag -m ""
  $ cd "$TESTTMP"
  $ git clone "$GIT_REPO_ORIGIN"
  Cloning into 'repo-git'...
  done.

# Capture all the known Git objects from the repo
  $ cd $GIT_REPO
  $ current_head=$(git rev-parse HEAD)
  $ git rev-list --objects --all | git cat-file --batch-check='%(objectname) %(objecttype) %(rest)' | sort > $TESTTMP/object_list

# Import it into Mononoke
  $ cd "$TESTTMP"
  $ quiet gitimport "$GIT_REPO" --derive-hg --generate-bookmarks full-repo

# Start up the Mononoke Git Service
  $ mononoke_git_service
# Clone the Git repo from Mononoke
  $ quiet git_client clone $MONONOKE_GIT_SERVICE_BASE_URL/$REPONAME.git
# Verify that we get the same Git repo back that we started with
  $ cd $REPONAME  
  $ git rev-list --objects --all | git cat-file --batch-check='%(objectname) %(objecttype) %(rest)' | sort > $TESTTMP/new_object_list
  $ diff -w $TESTTMP/new_object_list $TESTTMP/object_list

# Add more commits to the original git repo. Make sure the files added/modified in this commit
# create a cycle of deltas
  $ cd $GIT_REPO_ORIGIN
  $ echo "this is fileB.this is fileB.this is fileB.this is fileB.this is fileB.this is fileB.this is fileB.1.this is fileB.1" > fileB
  $ git add fileB
  $ git commit -qam "Modified fileB -> fileB.1"
  $ echo "this is fileB.this is fileB.this is fileB.this is fileB.this is fileB.this is fileB.this is fileB.this is fileB" > fileB
  $ git add fileB
  $ git commit -qam "Reverted fileB.1 -> fileB"
  $ echo "this is fileB.2 this is fileB.2 this is fileB.2 this is fileB.2 this is fileB.2 this is fileB.2 this is fileB.2.this is fileB.2" > fileB
  $ git add fileB
  $ git commit -qam "Modified fileB -> fileB.2"

  $ cd "$GIT_REPO"
  $ quiet git pull "$GIT_REPO_ORIGIN"
# Capture all the known Git objects from the repo
  $ cd $GIT_REPO_ORIGIN
  $ git rev-list --objects --all | git cat-file --batch-check='%(objectname) %(objecttype) %(rest)' | sort > $TESTTMP/object_list

# Import the newly added commits to Mononoke
  $ cd "$TESTTMP"
  $ quiet gitimport "$GIT_REPO_ORIGIN" --derive-hg --generate-bookmarks full-repo
# Pull the Git repo from Mononoke
  $ cd $REPONAME
# Wait for the warm bookmark cache to catch up with the latest changes
  $ wait_for_git_bookmark_move HEAD $current_head
  $ quiet git_client pull
# Verify that we get the same Git repo back that we started with
  $ git rev-list --objects --all | git cat-file --batch-check='%(objectname) %(objecttype) %(rest)' | sort > $TESTTMP/new_object_list
  $ diff -w $TESTTMP/new_object_list $TESTTMP/object_list
