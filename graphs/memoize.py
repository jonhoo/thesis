#!/usr/bin/env python

from ingest import ingest
try:
   import cPickle as pickle
except:
   import pickle
import sys

if __name__ == '__main__':
    source = ingest(sys.argv[1])
    with open(sys.argv[2], 'wb') as f:
        pickle.dump(source, f)
else:
    with open(sys.argv[1], 'rb') as f:
        source = pickle.load(f)
