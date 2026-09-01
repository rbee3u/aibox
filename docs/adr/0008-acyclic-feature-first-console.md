# Keep the Console feature-first and acyclic

The Console uses an enforced, acyclic feature-first graph with `app` as the
sole composition and browser-history owner. Business Features do not depend
directly on one another but may depend on the one-way `features/common` layer;
local workflows and query codecs keep feature state within that boundary.
