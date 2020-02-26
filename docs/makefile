BOB_SRCS=$(wildcard art/*.bob)
MSC_SRCS=$(wildcard art/*.msc)
MD_SRCS=$(wildcard src/*.md src/*/*.md)

SVG_IMGS=$(BOB_SRCS:art/%.bob=src/.gitbook/assets/%.svg) $(MSC_SRCS:art/%.msc=src/.gitbook/assets/%.svg)

TARGET=html/index.html
TEST_STAMP=src/tests.ok

all: $(TARGET)

svg: $(SVG_IMGS)

test: $(TEST_STAMP)

open: $(TEST_STAMP)
	mdbook build --open

watch: $(SVG_IMGS)
	mdbook watch

src/.gitbook/assets/%.svg: art/%.bob
	@mkdir -p $(@D)
	svgbob < $< > $@

src/.gitbook/assets/%.svg: art/%.msc
	@mkdir -p $(@D)
	mscgen -T svg -i $< -o $@

src/%.md: %.md
	@mkdir -p $(@D)
	@cp $< $@

$(TEST_STAMP): $(TARGET)
	mdbook test
	touch $@

$(TARGET): $(SVG_IMGS) $(MD_SRCS)
	mdbook build

clean:
	rm -f $(SVG_IMGS) src/tests.ok
	rm -rf html
