TEST(operation,
     geogCRS_to_geogCRS_context_concatenated_operation_Egypt1907_to_WGS84) {
    auto authFactory =
        AuthorityFactory::create(DatabaseContext::create(), "EPSG");
    auto ctxt = CoordinateOperationContext::create(authFactory, nullptr, 0.0);
    auto list = CoordinateOperationFactory::create()->createOperations(
        authFactory->createCoordinateReferenceSystem("4229"), // Egypt 1907
        authFactory->createCoordinateReferenceSystem("4326"), // WGS84
        ctxt);
    ASSERT_EQ(list.size(), 3U);
    // Concatenated operation
    EXPECT_EQ(list[1]->nameStr(), "Egypt 1907 to WGS 84 (2)");
    ASSERT_EQ(list[1]->coordinateOperationAccuracies().size(), 1U);
    EXPECT_EQ(list[1]->coordinateOperationAccuracies()[0]->value(), "6.0");
}
