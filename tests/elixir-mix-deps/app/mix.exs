defmodule App.MixProject do
  use Mix.Project

  def project do
    [
      app: :app,
      version: "0.1.0",
      deps: [{:jason, "~> 1.4"}]
    ]
  end
end
