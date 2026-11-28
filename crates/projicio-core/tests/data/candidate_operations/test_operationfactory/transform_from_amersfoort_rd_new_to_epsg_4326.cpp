TEST(operation, transform_from_amersfoort_rd_new_to_epsg_4326) {
    auto authFactory =
        AuthorityFactory::create(DatabaseContext::create(), "EPSG");
    auto ctxt = CoordinateOperationContext::create(authFactory, nullptr, 0.0);
    auto list = CoordinateOperationFactory::create()->createOperations(
        authFactory->createCoordinateReferenceSystem("28992"),
        authFactory->createCoordinateReferenceSystem("4326"), ctxt);
    ASSERT_EQ(list.size(), 2U);
    // The order matters: "Amersfoort to WGS 84 (4)" replaces "Amersfoort to WGS
    // 84 (3)"
    EXPECT_EQ(list[0]->nameStr(),
              "Inverse of RD New + Amersfoort to WGS 84 (4)");
    EXPECT_EQ(list[1]->nameStr(),
              "Inverse of RD New + Amersfoort to WGS 84 (3)");
}

