FROM debezium/connect:2.6

ARG CONFLUENT_VERSION=7.6.0
RUN curl -sL "https://api.hub.confluent.io/api/plugins/confluentinc/kafka-connect-avro-converter/versions/${CONFLUENT_VERSION}/archive" \
         -o /tmp/avro-converter.zip && \
    unzip -q /tmp/avro-converter.zip -d /tmp/avro-converter && \
    mkdir -p /kafka/connect/confluent-avro-converter && \
    cp /tmp/avro-converter/confluentinc-kafka-connect-avro-converter-${CONFLUENT_VERSION}/lib/*.jar \
       /kafka/connect/confluent-avro-converter/ && \
    rm -rf /tmp/avro-converter.zip /tmp/avro-converter
