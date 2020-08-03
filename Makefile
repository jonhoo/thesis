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

graphs/source.pickle: graphs/extract-hist/src/main.rs graphs/ingest.py graphs/memoize.py \
                      $(wildcard benchmarks/orchestration/*.log) \
                      $(wildcard benchmarks/orchestration/*.hist) \
                      $(wildcard benchmarks/orchestration/*.json)
	graphs/memoize.py benchmarks/orchestration/ $@

graphs/lobsters-memlimit-cdf.pdf: graphs/source.pickle graphs/common.py graphs/lobsters-memlimit-cdf.py
	python graphs/lobsters-memlimit-cdf.py graphs/source.pickle benchmarks/orchestration/ graphs/lobsters-memlimit-cdf

graphs/lobsters-durability-cdf.pdf: graphs/source.pickle graphs/common.py graphs/lobsters-durability-cdf.py
	python graphs/lobsters-durability-cdf.py graphs/source.pickle benchmarks/orchestration/ graphs/lobsters-durability-cdf

graphs/vote-formula.pdf: graphs/source.pickle graphs/common.py formula/src/main.rs graphs/vote-formula.py
	python graphs/vote-formula.py graphs/source.pickle graphs/vote-formula

graphs/%.pdf: graphs/source.pickle graphs/common.py graphs/%.py
	python graphs/$*.py graphs/source.pickle graphs/$*
