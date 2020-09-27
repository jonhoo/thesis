#!/bin/sh
rm ../results/{lobsters,lobsters-mysql,vote,vote-migration,vote-nojoin,vote-redis}/*.{json,log,hist}
mv lobsters-mysql* ../results/lobsters-mysql/
mv lobsters-direct* ../results/lobsters/
mv redis.* ../results/vote-redis/
mv *_nj.* ../results/vote-nojoin/
mv vote-* ../results/vote-migration/
mv partial* ../results/vote/
mv full* ../results/vote/
mv run.log ../results/
