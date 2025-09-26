build:
	docker build . -t $(DOCKER_TAG)

push:
	docker push $(DOCKER_TAG)

run:
	docker run $(DOCKER_TAG) server

.PHONY: build push run