.PHONY: all
all:
	$(MAKE) sources
	$(MAKE) thesis.pdf

thesis.pdf: titlepage.pdf abstract.pdf \
            thesis.tex bibliography.bib \
	    evaluation.tex \
	    graphs/lobsters-throughput.pdf \
	    graphs/lobsters-memory.pdf \
	    graphs/lobsters-memlimit-cdf.pdf \
	    graphs/lobsters-durability-cdf.pdf \
	    graphs/lobsters-timeline.pdf \
	    graphs/vote-formula.pdf \
	    graphs/vote-migration.pdf \
	    graphs/vote-throughput-memlimit.pdf \
	    graphs/vote-redis.pdf
	latexmk -shell-escape -pdf thesis.tex

proposal.pdf: 000-proposal.tex bibliography.bib \
              jfrg-thesis-proposal-agreement-robert.pdf \
              jfrg-thesis-proposal-agreement-frans.pdf \
              jfrg-thesis-proposal-agreement-sam.pdf \
              jfrg-thesis-proposal-agreement-malte.pdf \
              signature.pdf
	latexmk -pdf 000-proposal.tex
	cp 000-proposal.pdf $@

%.pdf: %.tex
	latexmk -pdf $<

RESULTS = lobsters lobsters-mysql vote vote-migration vote-redis vote-formula

.PHONY: sources $(RESULTS)

sources: $(RESULTS)

$(RESULTS):
	$(MAKE) -C $(addprefix benchmarks/results/, $@)

benchmarks/results/%/parsed.pickle: sources

graphs/lobsters-%.pdf: graphs/common.py benchmarks/results/lobsters/parsed.pickle graphs/lobsters-%.py
	python graphs/lobsters-$*.py graphs/lobsters-$*

graphs/lobsters-throughput.pdf: graphs/common.py graphs/lobsters-throughput.py \
	benchmarks/results/lobsters/parsed.pickle \
	benchmarks/results/lobsters-mysql/parsed.pickle
	python graphs/lobsters-throughput.py graphs/lobsters-throughput

graphs/vote-migration.pdf: graphs/common.py benchmarks/results/vote-migration/parsed.pickle graphs/vote-migration.py
	python graphs/vote-migration.py graphs/vote-migration

graphs/vote-redis.pdf: graphs/common.py graphs/vote-redis.py \
	benchmarks/results/vote-nojoin/parsed.pickle \
	benchmarks/results/vote-redis/parsed.pickle
	python graphs/vote-redis.py graphs/vote-redis

graphs/vote-formula.pdf: graphs/common.py graphs/vote-formula.py benchmarks/results/vote-formula/results.log
	python graphs/vote-formula.py graphs/vote-formula

graphs/vote-%.pdf: graphs/common.py benchmarks/results/vote/parsed.pickle graphs/vote-%.py
	python graphs/vote-$*.py graphs/vote-$*
