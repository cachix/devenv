As ``devenv`` downloads dependencies and builds software,
your disk storage will grow.

``devenv`` will never delete something that it has built,
that's an explicit action.

Running ``devenv gc`` will go through everything you've built so far
and delete everything that's currently not the latest successful invocation
of any ``devenv`` command per folder.
