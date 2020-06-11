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
	    evaluation.tex
	latexmk -pdf thesis.tex
