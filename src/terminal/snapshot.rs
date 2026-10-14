//! Copy of the visible cells, cursor and selection, taken under a short `Term` lock so that
//! rendering never holds the lock.
