{ inputs, ... }:
{
  env.INPUT_NAR_HASH = inputs.metadata.metadata.narHash;
  env.INPUT_LAST_MODIFIED = toString inputs.metadata.metadata.lastModified;
}
