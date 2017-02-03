set -e
if ([ "$TRAVIS_BRANCH" == "master" ] || [ ! -z "$TRAVIS_TAG" ]) &&
       [ "$TRAVIS_PULL_REQUEST" == "false" ] &&
       [ "$TRAVIS_RUST_VERSION" == "nightly" ]; then
  cargo doc
  sudo pip install ghp-import
  ghp-import -n target/doc
  git push -qf https://${TOKEN}@github.com/${TRAVIS_REPO_SLUG}.git gh-pages
  echo "Doc upload finished"
fi

