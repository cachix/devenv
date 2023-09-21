set -xe 
pip install poetry
poetry install
pip install git+https://${GH_TOKEN}@github.com/squidfunk/mkdocs-material-insiders.git@9.1.5-insiders-4.32.4
poetry run -- mkdocs build --config-file mkdocs.insiders.yml
