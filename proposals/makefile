BOB_SRCS=$(wildcard art/*.bob)
MD_SRCS=$(wildcard src/*.md)

SVG_IMGS=$(BOB_SRCS:art/%.bob=src/img/%.svg)

all: html/index.html

test: src/tests.ok

open: all
	mdbook build --open

watch: $(SVG_IMGS)
	mdbook watch

src/img/%.svg: art/%.bob
	@mkdir -p $(@D)
	svgbob < $< > $@

src/%.md: %.md
	@mkdir -p $(@D)
	@cp $< $@

src/tests.ok: $(SVG_IMGS) $(MD_SRCS)
	mdbook test
	touch $@

html/index.html: src/tests.ok
	mdbook build

clean:
	rm -f $(SVG_IMGS) src/tests.ok
	rm -rf html
