set -xe
pip3 install poetry
poetry install
pip3 install git+https://${GH_TOKEN}@github.com/squidfunk/mkdocs-material-insiders.git@9.4.0-insiders-4.42.0
mkdocs build --config-file mkdocs.insiders.yml
