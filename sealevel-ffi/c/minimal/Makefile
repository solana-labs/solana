CCFLAGS=-Wall -I../../ -L../../../target/debug -lsealevel

minimal: minimal.c
	$(CC) $(CCFLAGS) -o $@ $<

.PHONY: clean
clean:
	rm -f minimal
