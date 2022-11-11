With time, your disk usage will increase as ``devenv`` downloads dependencies and builds software.

``devenv`` will never delete something that it has built.

Running ``devenv gc`` will go through everything you've built so far
and delete anything that's currently not the latest successful invocation
of any ``devenv`` command per folder.
