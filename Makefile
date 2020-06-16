%.pdf: %.tex
	latexmk -pdf $<

proposal.pdf: 000-proposal.tex bibliography.bib \
              jfrg-thesis-proposal-agreement-robert.pdf \
              jfrg-thesis-proposal-agreement-frans.pdf \
              jfrg-thesis-proposal-agreement-sam.pdf \
              jfrg-thesis-proposal-agreement-malte.pdf \
              signature.pdf
	latexmk -pdf 000-proposal.tex
	cp 000-proposal.pdf $@

thesis.pdf: titlepage.pdf abstract.pdf \
            thesis.tex bibliography.bib \
	    evaluation.tex \
	    graphs/lobsters-memory.pdf
	latexmk -pdf thesis.tex

graphs/source.pickle: graphs/memoize.py \
                      $(wildcard benchmarks/orchestration/ex/*.log) \
                      $(wildcard benchmarks/orchestration/ex/*.hist) \
                      $(wildcard benchmarks/orchestration/ex/*.json)
	graphs/memoize.py benchmarks/orchestration/ex/ $@

graphs/%.pdf: graphs/source.pickle graphs/common.py graphs/%.py
	python graphs/$*.py graphs/source.pickle graphs/$*
