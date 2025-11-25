.PHONY: all
all: example

.PHONY: install
install: example
	install -Dm755 $< $(DESTDIR)/bin/$<

.PHONY: clean
clean:
	$(RM) example
